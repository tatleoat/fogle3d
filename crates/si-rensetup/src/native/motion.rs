// SPDX-License-Identifier: MIT
//! The scene brought to a frame's time on the image: `si-motion`'s port of
//! `SI_sceneUpdateItems` (finding 0017), called as `si-render`'s `SI_update` calls it
//! (finding 0012, section 21), with every routine the update calls outside `si-motion`
//! handed to the port another crate has on the image, by the host method's name, and every
//! other answered 0 and counted (finding 0023, section 21).
//!
//! Nothing in this file reproduces a function of the original: it wires ported functions
//! together. Unlike `re/tools/si-motion/evalcheck`, it has no stand-ins of its own: the
//! `edtools.c` look-ups that tool writes for its check are `si-scene`'s ports
//! (`si_scene::edtools`, compared with the emulated original), the inverse kinematics and
//! the constraints' directions are `si-kine`'s (finding 0022), the `model.c` setters
//! `si-model`'s (finding 0015, sixth pass), the envelope `si-deform`'s (finding 0029).

use si_mem::{Mem, Ptr32};
use si_motion::host::{Arg, Ret, Trace};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

mod animbool;
mod dynachains;
pub use dynachains::{dynamic_chains_update, loader_answer};

/// Calls the update made, by the host method's name: (answered by a port, answered 0).
pub type Calls = BTreeMap<&'static str, (usize, usize)>;

fn word(a: &Arg) -> u32 {
    match a {
        Arg::I(v) | Arg::F(v) => *v,
        Arg::D(v) => *v as u32,
    }
}

/// `si-model`'s, `si-spline`'s and `si-dict`'s outside world during the update: every
/// routine they call answers 0 (as the render command's world answers them: none of their
/// callees is on the image), counted under the caller's name.
struct Quiet<'a>(&'a RefCell<Calls>, &'static str);
impl si_model::model_image::Outside for Quiet<'_> {
    fn call(&mut self, _: &mut Mem, _: &si_model::model_image::Callee, _: &[u32]) -> u32 {
        self.0.borrow_mut().entry(self.1).or_default().1 += 1;
        0
    }
}
impl si_spline::spline_image::Outside for Quiet<'_> {}
impl si_mem::Outside for Quiet<'_> {
    fn call(&mut self, _: &mut Mem, _: si_mem::Routine, _: &[u32]) -> si_mem::Ret {
        self.0.borrow_mut().entry(self.1).or_default().1 += 1;
        si_mem::Ret::NONE
    }
}
impl si_dict::Outside for Quiet<'_> {
    fn call(&mut self, _: &mut Mem, _: &'static si_dict::Routine, _: &[u32]) -> u32 {
        self.0.borrow_mut().entry(self.1).or_default().1 += 1;
        0
    }
}

/// `si-model`'s outside world for `SI_modelUpdateSonState`: a node whose geometry another
/// node's branch deformation holds (`latdep`, +0x44, not the node) has its definition
/// readjusted with the new `def_matrix` (`SI_modelReadjustDefinition`, `si-model`'s, its
/// calls out as [`ReadjustSide`] answers them; a hook when `RENSETUP_NO_READJUST=1`); every
/// other routine answers 0, counted.
struct SonSide<'a>(&'a RefCell<Calls>);
impl si_model::model_image::Outside for SonSide<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        if callee.addr == si_model::model_image::callee::SI_MODEL_READJUST_DEFINITION.addr && readjust_on() {
            si_model::model_image::readjust::readjust_definition(m, &mut ReadjustSide(self.0), Ptr32::new(a.first().copied().unwrap_or(0)));
            self.0.borrow_mut().entry(callee.name).or_default().0 += 1;
            return 0;
        }
        Quiet(self.0, "callees of SI_modelUpdateSonState").call(m, callee, a)
    }
}

/// routing6: the model image's wave tests and the patch's disposal, which the model code reaches
/// through its outside world: `SI_modelNodeWave (0x084c7610)`, `SI_modelBranchWave (0x084c75dc)`
/// (si-model's ports on the image) and `SI_patchDispose (0x0847a530)` (si-spline's, the pointer to
/// the patch pointer as its argument). `RENSETUP_NO_ROUTING6=1` leaves them hooks (answered 0).
/// Nothing here reproduces a function of the original; it connects ported ones.
pub(super) fn routing6_model_ports(m: &mut Mem, callee: &si_model::model_image::Callee, a: &[u32]) -> Option<u32> {
    use si_model::model_image::callee as mc;
    if !routing6_on() {
        return None;
    }
    let w = |k: usize| a.get(k).copied().unwrap_or(0);
    if callee.addr == mc::SI_MODEL_NODE_WAVE.addr {
        Some(si_model::model_image::readjust::node_wave(m, Ptr32::new(w(0))))
    } else if callee.addr == mc::SI_MODEL_BRANCH_WAVE.addr {
        Some(si_model::model_image::readjust::branch_wave(m, Ptr32::new(w(0))))
    } else if callee.addr == mc::SI_PATCH_DISPOSE.addr {
        si_spline::patch_image::patch_dispose(m, w(0));
        Some(0)
    } else {
        None
    }
}

/// On (the default): `si_symmetryVector` is this crate's port; `RENSETUP_NO_SYMVEC=1` leaves the host method a hook.
fn symvec_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_SYMVEC").is_none())
}

/// On (the default): routing6's routes ([`routing6_model_ports`], the host method `dyna_algo_type_set`);
/// `RENSETUP_NO_ROUTING6=1` leaves them hooks answered 0, counted, as before.
fn routing6_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_ROUTING6").is_none())
}

/// `si-model`'s outside world when the update calls its setters: `SI_findGlData` is
/// `si-view`'s port, `SI_removeTriangle` (the envelope's touch) `si-model`'s
/// ([`remove_triangle`]); every other routine answers 0 and is counted.
struct ModelSide<'a>(&'a RefCell<Calls>);
impl si_model::model_image::Outside for ModelSide<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, args: &[u32]) -> u32 {
        if callee.name == si_model::model_image::callee::SI_FIND_GL_DATA.name {
            let w = |k: usize| args.get(k).copied().unwrap_or(0);
            return si_view::graflist::find_gl_data(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32).0;
        }
        if callee.addr == si_model::model_image::callee::SI_REMOVE_TRIANGLE.addr && deform_lookups_on() {
            remove_triangle(m, self.0, args);
            self.0.borrow_mut().entry(callee.name).or_default().0 += 1;
            return 0;
        }
        // si-gen: SI_genTreeDestroy (0x08515848), the bounding-box tree a NURBS surface's disposal
        // frees (its elements through SI_bezPatchDestroy, si-spline's); `RENSETUP_NO_GEN_TREE=1` keeps it a hook
        if callee.addr == 0x0851_5848 && gen_tree_on() {
            let w = |k: usize| args.get(k).copied().unwrap_or(0);
            si_gen::tree::destroy(m, &mut GenSide(self.0), w(0), w(1));
            self.0.borrow_mut().entry(callee.name).or_default().0 += 1;
            return 0;
        }
        // si-nurbs: SI_nurbsSrfForceCapping (0x084f0134), the capping of a surface whose
        // point moved ([`si_nurbs::image::nurbs_capping`]); its own point moves go back
        // through this host (markDirtyCcrvs stays answered 0, counted)
        if callee.addr == si_nurbs::image::SI_NURBS_SRF_FORCE_CAPPING.addr && nurbs_fit_on() {
            let w = |k: usize| args.get(k).copied().unwrap_or(0);
            si_nurbs::image::srf_force_capping(m, &mut si_scene::envside::NurbsSide(&mut ModelSide(self.0)), w(0), w(1) as i16, w(2) as i16);
            self.0.borrow_mut().entry(callee.name).or_default().0 += 1;
            return 0;
        }
        // routeC: markDirtyCcrvs [soft 0x084ea5ec] (si-nurbs' port on the image; a refusal stays a hook)
        if callee.addr == si_nurbs::image::MARK_DIRTY_CCRVS.addr && nurbs_fit_on() {
            let w = |k: usize| args.get(k).copied().unwrap_or(0);
            if si_nurbs::geom5::mark_dirty_ccrvs(m, w(0), w(1) as i32).is_ok() {
                self.0.borrow_mut().entry(callee.name).or_default().0 += 1;
                return 0;
            }
        }
        if let Some(v) = routing6_model_ports(m, callee, args) {
            self.0.borrow_mut().entry(callee.name).or_default().0 += 1;
            return v;
        }
        self.0.borrow_mut().entry(callee.name).or_default().1 += 1;
        0
    }
}

/// On (the default); `RENSETUP_NO_GEN_TREE=1` leaves `SI_genTreeDestroy` a hook (answered 0, counted), as before routeF.
fn gen_tree_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_GEN_TREE").is_none())
}

/// The callbacks of `SI_genTreeDestroy`: its elements' destructor `SI_bezPatchDestroy
/// (0x08503504)` is si-spline's port ([`si_spline::bez_image::destroy`]); any other callback answers 0, counted.
struct GenSide<'a>(&'a RefCell<Calls>);
impl si_mem::Outside for GenSide<'_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        if r.addr == 0x0850_3504 {
            si_spline::bez_image::destroy(m, a.first().copied().unwrap_or(0));
            return si_mem::Ret::NONE;
        }
        self.0.borrow_mut().entry("callbacks of SI_genTreeDestroy").or_default().1 += 1;
        si_mem::Ret::NONE
    }
}

/// `si-model`'s outside world for `SI_revolutionUpdate` and `SI_zipUpdate` (finding 0015, the
/// last sections): `createModelFromRevolution` is `si-model`'s port (a kind 2 or 9 source,
/// not ported, is left as it is, counted), `SI_modelAllocate` is `model_b`'s, the model's
/// touch (`SI_modelTouchGeometry`) `si-model`'s with [`ReadjustSide`]'s calls out and
/// `SI_findGlData` `si-view`'s; the status line, `SI_mdlStatusMessages`, `sleep`, the
/// relation's relaxing (`SI_edRelaxRelFromDelModel`) and the zip dialog's sliders do nothing
/// in a batch run (answered 0, counted). Answered 0 as today, counted: `meshRevolveSpline`
/// (a revolved spline's mesh), the four NURBS zip routines (`SI_nurbsSrfCopy`,
/// `initNrbSrfMaxValue`, `refreshNrbSrfZip`, `SI_nurbsSrfDispose`) and every other routine.
/// On (the default); `RENSETUP_NO_MODELER_REV=1` routes `createModelFromRevolution` to si-model's partial port again.
fn modeler_rev_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_MODELER_REV").is_none())
}

/// si-modeler's outside world for `createModelFromRevolution`: every routine it calls goes to [`RevZipSide`]'s answers by
/// address (the calls are counted there; an unported one answers 0, counted).
struct ModelerRevSide<'a>(&'a RefCell<Calls>);
impl si_mem::Outside for ModelerRevSide<'_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        let v = si_model::model_image::Outside::call(&mut RevZipSide(self.0), m, &si_model::model_image::Callee { name: r.name, addr: r.addr }, a);
        si_mem::Ret::eax(v)
    }
}

/// `RENSETUP_NO_REVOLUTION_ZIP=1` answers both updates 0 again.
struct RevZipSide<'a>(&'a RefCell<Calls>);
impl si_model::model_image::Outside for RevZipSide<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        use si_model::model_image::callee as mc;
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let addr = callee.addr;
        let answer = if addr == 0x0819_5b7c {
            // createModelFromRevolution (src, axis, cell, rev): si-modeler's full port (kinds 2, 6 and 9) unless
            // `RENSETUP_NO_MODELER_REV=1`, which keeps si-model's partial one (kind 6 only; 2 and 9 left, counted)
            if modeler_rev_on() {
                Some(si_modeler::effects::revolve_four::create_model_from_revolution(m, &mut ModelerRevSide(calls), w(0), w(1), w(2), w(3)) as u32)
            } else {
                si_model::model_image::tree::create_model_from_revolution(m, &mut RevZipSide(calls), Ptr32::new(w(0)), w(1), w(2), w(3)).map(|v| v as u32)
            }
        } else if addr == mc::SI_MODEL_ALLOCATE.addr {
            Some(super::model_b::model_allocate(m, Ptr32::new(w(0))).0)
        } else if addr == mc::SI_MODEL_TOUCH_GEOMETRY.addr {
            si_model::model_image::touch::touch_geometry(m, &mut ReadjustSide(calls), Ptr32::new(w(0)));
            Some(0)
        } else if addr == mc::SI_FIND_GL_DATA.addr {
            Some(si_view::graflist::find_gl_data(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32).0)
        } else if addr == mc::SI_MODEL_READJUST_DEFINITION.addr && readjust_on() {
            si_model::model_image::readjust::readjust_definition(m, &mut ReadjustSide(calls), Ptr32::new(w(0)));
            Some(0)
        } else if addr == mc::SI_MODEL_NODE_WAVE.addr {
            Some(si_model::model_image::readjust::node_wave(m, Ptr32::new(w(0))))
        } else if addr == mc::SI_MODEL_BRANCH_WAVE.addr {
            Some(si_model::model_image::readjust::branch_wave(m, Ptr32::new(w(0))))
        } else if addr == mc::SI_PATCH_UPDATE_SURFACE.addr {
            si_spline::patch_image::patch_update_surface(m, Ptr32::new(w(0)));
            Some(0)
        } else if addr == mc::SI_PATCH_DISPOSE.addr && routing6_on() {
            si_spline::patch_image::patch_dispose(m, w(0));
            Some(0)
        } else {
            None
        };
        let mut c = calls.borrow_mut();
        let e = c.entry(callee.name).or_default();
        match answer {
            Some(v) => {
                e.0 += 1;
                v
            }
            None => {
                e.1 += 1;
                0
            }
        }
    }
}

/// `RENSETUP_NO_REVOLUTION_ZIP=1` answers `SI_revolutionUpdate` and `SI_zipUpdate` 0 again.
fn revolution_zip_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_REVOLUTION_ZIP").is_none())
}

/// `si-deform`'s outside world during the update: the cluster's element
/// (`SI_edGetElemFromCluster`, `si-scene`'s) and the count of its centre relations
/// (`SI_elemdictRangeRelationsCount`, `si-dict`'s); for the envelope (`SI_envelopCompUpdate`,
/// finding 0029, third pass) the ports `si_scene::envside::envelope_call` names (the model
/// tree's updates, the meshes' copies and normals, the spline and patch copies, …); for
/// `SI_postAnimTouch` the readjusting (`SI_modelReadjustDefinition`, `si-model`'s, its calls
/// out as [`ReadjustSide`] answers them; a hook when `RENSETUP_NO_READJUST=1`) and the walk
/// of the subtree (`SI_modelNextSon`, `si-model`'s); every other routine answers 0, counted.
struct DeformSide<'a>(&'a RefCell<Calls>);
impl si_deform::outside::Outside for DeformSide<'_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_deform::outside::Routine, a: &[u32]) -> u32 {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        match r.addr {
            0x080f_d510 => si_scene::edtools::ed_get_elem_from_cluster(&mut Lookups(self.0), m, w(0), Ptr32::new(w(1))).0,
            0x0813_5770 => si_dict::elemrel::range_relations_count(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32,
            0x0846_e340 if readjust_on() => {
                si_model::model_image::readjust::readjust_definition(m, &mut ReadjustSide(self.0), Ptr32::new(w(0)));
                0
            }
            0x0846_d70c => si_model::model_image::tree::next_son(m, Ptr32::new(w(0)), Ptr32::new(w(1))).addr(),
            // SI_genTreeDestroy (0x08515848): si-gen's port (the deformation's disposals of a NURBS surface reach it here)
            0x0851_5848 if gen_tree_on() => {
                si_gen::tree::destroy(m, &mut GenSide(self.0), w(0), w(1));
                self.0.borrow_mut().entry(r.name).or_default().0 += 1;
                0
            }
            // SI_removeTriangle (0x085efc38), grTriangle.c: si-model's port, as ModelSide routes it
            // (the refresh's envelope reaches it through here; `RENSETUP_NO_DEFORM_LOOKUPS=1` keeps it a hook)
            0x085e_fc38 if deform_lookups_on() => {
                remove_triangle(m, self.0, a);
                self.0.borrow_mut().entry(r.name).or_default().0 += 1;
                0
            }
            // SI_splineDistanceToValueFrenet (0x0848f6f8), si-spline's: the point and frame
            // along a spline fitting's curve (the frames built on first use)
            0x0848_f6f8 => {
                si_spline::frenet_image::spline_distance_to_value_frenet(m, &mut Quiet(self.0, "callees of SI_splineDistanceToValueFrenet"), Ptr32::new(w(0)), f32::from_bits(w(1)), w(2), w(3), w(4));
                self.0.borrow_mut().entry(r.name).or_default().0 += 1;
                0
            }
            _ => match si_scene::envside::envelope_call(&mut EnvHost(self.0), m, r.addr, a) {
                Some(v) => {
                    self.0.borrow_mut().entry(r.name).or_default().0 += 1;
                    v
                }
                None => {
                    self.0.borrow_mut().entry(r.name).or_default().1 += 1;
                    0
                }
            },
        }
    }
    fn drand48(&mut self) -> f64 {
        0.0
    }
}

/// The host of `si_scene::envside::envelope_call`: the dictionary's look-ups as [`Lookups`]
/// answers them, `si-model`'s calls out as [`ModelSide`].
struct EnvHost<'a>(&'a RefCell<Calls>);
impl si_scene::edtools::ByAddr for EnvHost<'_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
        Lookups(self.0).call_addr(m, f, a)
    }
}
impl si_model::model_image::Outside for EnvHost<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, args: &[u32]) -> u32 {
        ModelSide(self.0).call(m, callee, args)
    }
}

/// `si-model`'s outside world when the update readjusts a definition
/// (`SI_modelReadjustDefinition`): the lattice's deformation (`SI_latticeTransform`) and the
/// envelope's switch (`SI_envelopActive`) are `si-deform`'s ports; the update node's search
/// (`SI_edFindUpdateNode`), its active time (`SI_checkActiveTime`) and the cluster
/// constraint (`SI_animMdlClusterCnsUpdate`) `si-motion`'s, the last two with an update
/// host of their own ([`nested`]); a spline fitting (`SI_splineFit`) `si-model`'s, for the
/// fittings whose callees are on the image ([`spline_fit_missing`] names the rest, left as
/// they are, counted; `RENSETUP_NO_SPLINE_FIT=1` answers it 0 again), its point and frame
/// along the curve (`SI_curveDistanceToValueFrenet`) `si-deform`'s, the spline's
/// `si-spline`'s ([`DeformSide`]); the quick stretch (`SI_animQuickStretchUpdate`)
/// `si-kine`'s, except for a face or NURBS model that would need a working copy, which no
/// crate has on the image (left as it is, counted); the spline's touch `si-spline`'s;
/// `si_meshTouch` is an empty function in this build; the patch's points
/// (`SI_patchTransferPoints`) go to `si-spline`'s adapter (a patch without display arrays,
/// as the loader leaves them, gets its points only, counted), and the render data's
/// disposal (`SI_removeTriangle`) to `si-model`'s port ([`remove_triangle`]), hooks when
/// `RENSETUP_NO_DEFORM_LOOKUPS=1`. The rest as the envelope's table
/// answers it ([`EnvHost`]: the element of a model, the meshes' and splines' copies, …),
/// else 0, counted.
struct ReadjustSide<'a>(&'a RefCell<Calls>);
impl si_model::model_image::Outside for ReadjustSide<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        use si_model::model_image::{callee as mc, readjust::callees as rc};
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let addr = callee.addr;
        let answer = if addr == rc::SI_WAVE_TRANSFORM.addr && wave_transform_on() {
            si_scene::wave_sys::wave_transform(&mut WaveSide(calls), m, w(0), w(1), w(2) as i16, f32::from_bits(w(3)), w(4) as i16);
            Some(0)
        } else if addr == rc::SI_LATTICE_TRANSFORM.addr {
            si_deform::lattice::lattice_transform(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0)), w(1) as i32, w(2), w(3));
            Some(0)
        } else if addr == rc::SI_SPLINE_FIT.addr && spline_fit_on() {
            if let Some(why) = spline_fit_missing(m, w(0), w(1) as i32) {
                calls.borrow_mut().entry(why).or_default().1 += 1;
                return 0;
            }
            si_model::model_image::fit::spline_fit(m, &mut ReadjustSide(calls), w(0), w(1) as i32, w(2), w(3));
            Some(0)
        } else if addr == si_model::model_image::fit::callees::SI_CURVE_DISTANCE_TO_VALUE_FRENET.addr {
            si_deform::deformation::curve_distance_to_value_frenet(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), w(0), f32::from_bits(w(1)), w(2), w(3), w(4));
            Some(0)
        } else if addr == mc::SI_NURBS_CRV_APPLY_MATRIX.addr && nurbs_fit_on() {
            si_nurbs::image::crv_apply_matrix(m, w(0), w(1), w(2) as i32);
            Some(0)
        } else if addr == mc::SI_NURBS_SRF_APPLY_MATRIX.addr && nurbs_fit_on() {
            si_nurbs::image::srf_apply_matrix(m, &mut si_scene::envside::NurbsSide(&mut ModelSide(calls)), w(0), w(1), w(2) as i32);
            Some(0)
        } else if addr == rc::SI_PATCH_FIT.addr {
            // SI_patchFit (0x0846f444) (all six kinds are written in si-model)
            si_model::model_image::patch_fit::patch_fit(m, &mut ReadjustSide(calls), w(0), w(1) as i32, w(2), w(3));
            Some(0)
        } else if addr == mc::SI_ENVELOP_ACTIVE.addr {
            Some(si_deform::envtools::envelop_active(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0))) as u32)
        } else if addr == rc::SI_ED_FIND_UPDATE_NODE.addr {
            Some(si_motion::update::ed_find_update_node(m, w(0), w(1), w(2) as i32, w(3)))
        } else if addr == rc::SI_CHECK_ACTIVE_TIME.addr {
            let at = nested(calls, |h| si_motion::update::check_active_time(m, h, w(0), f32::from_bits(w(1))));
            Some(match at {
                Some(t) => {
                    if w(2) != 0 {
                        m.set_f32(w(2), t);
                    }
                    1
                }
                None => 0,
            })
        } else if addr == rc::SI_ANIM_MDL_CLUSTER_CNS_UPDATE.addr {
            nested(calls, |h| si_motion::update::constr::anim_mdl_cluster_cns_update(m, h, w(0), w(1) as i16, w(2) as i16, w(3), f32::from_bits(w(4))));
            Some(0)
        } else if addr == rc::SI_ANIM_QUICK_STRETCH_UPDATE.addr {
            if let Some(why) = qstretch_copy_missing(m, w(0)) {
                calls.borrow_mut().entry(why).or_default().1 += 1;
                return 0;
            }
            si_kine::image::qstretch::anim_quick_stretch_update(m, &mut KineOutside(calls), w(0), f32::from_bits(w(1)));
            Some(0)
        } else if addr == mc::SI_ELEMDICT_FIND_RELATED_ELEM.addr {
            Some(si_dict::elemrel::find_related_elem(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, Ptr32::new(w(6)), Ptr32::new(w(7))).0)
        } else if addr == mc::SI_SPLINE_TOUCH.addr {
            si_spline::spline_image::spline_touch(m, &mut Quiet(calls, "callees of SI_splineTouch"), Ptr32::new(w(0)));
            Some(0)
        } else if addr == mc::SI_MESH_TOUCH.addr {
            Some(0)
        } else if addr == mc::SI_REMOVE_TRIANGLE.addr && deform_lookups_on() {
            remove_triangle(m, calls, a);
            Some(0)
        } else if addr == mc::SI_PATCH_TRANSFER_POINTS.addr && deform_lookups_on() {
            if !si_spline::patch_image::patch_transfer_points(m, Ptr32::new(w(0)), Ptr32::new(w(1))) {
                calls.borrow_mut().entry("SI_patchTransferPoints (display curves not on the image: the points only)").or_default().1 += 1;
            }
            Some(0)
        } else if addr == 0x0851_5848 && gen_tree_on() {
            si_gen::tree::destroy(m, &mut GenSide(calls), w(0), w(1));
            Some(0)
        } else if let Some(v) = routing6_model_ports(m, callee, a) {
            Some(v)
        } else {
            si_scene::envside::envelope_call(&mut EnvHost(calls), m, addr, a)
        };
        let mut c = calls.borrow_mut();
        let e = c.entry(callee.name).or_default();
        match answer {
            Some(v) => {
                e.0 += 1;
                v
            }
            None => {
                e.1 += 1;
                0
            }
        }
    }
}

/// Why `SI_splineFit(fit, kind, …)` (`si-model`'s `fit::spline_fit`) is not run here, when
/// it is not: the fitting follows a NURBS curve (kind 9 at +0x30 of the node at `fit` + 4),
/// whose point and frame (`SI_nurbsDistanceToValueFrenet (0x08503068)`) no crate has on the
/// image; or, along a spline, the definition is a NURBS curve or surface (9, 10), whose last
/// step (`SI_nurbsCrvApplyMatrix (0x084e9750)`, `SI_nurbsSrfApplyMatrix (0x084e9644)`) is not
/// on the image. Such a definition is left as it is (as before the routing).
pub(crate) fn spline_fit_missing(m: &Mem, fit: u32, kind: i32) -> Option<&'static str> {
    // [soft 0x084e9644 SI_nurbsSrfApplyMatrix] and [soft 0x084e9750 SI_nurbsCrvApplyMatrix] are
    // `si-nurbs`' image forms and routed in [`ReadjustSide`]: a spline definition of kind 9 or
    // 10 goes through the fitting (`RENSETUP_NO_NURBS_FIT=1` brings the refusal back)
    if nurbs_fit_on() {
        let curve = m.i32(m.u32(fit.wrapping_add(4)).wrapping_add(0x30));
        return if curve == 9 { Some("SI_splineFit (along a NURBS curve: SI_nurbsDistanceToValueFrenet is not on the image)") } else { None };
    }
    let curve = m.i32(m.u32(fit.wrapping_add(4)).wrapping_add(0x30));
    if curve == 9 {
        return Some("SI_splineFit (along a NURBS curve: SI_nurbsDistanceToValueFrenet is not on the image)");
    }
    if curve != 6 {
        return None;
    }
    match kind {
        9 | 10 => Some("SI_splineFit (a NURBS curve or surface: SI_nurbsCrvApplyMatrix / SI_nurbsSrfApplyMatrix are not on the image)"),
        _ => None,
    }
}

/// On (the default): `SI_nurbsCrvApplyMatrix`, `SI_nurbsSrfApplyMatrix` and
/// `SI_nurbsSrfForceCapping` are `si-nurbs`' ports on the image (finding 0025, section 17);
/// `RENSETUP_NO_NURBS_FIT=1` answers them 0 again, counted.
fn nurbs_fit_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_NURBS_FIT").is_none())
}

/// On (the default): the readjusting's `SI_splineFit` is `si-model`'s port for the kinds
/// [`spline_fit_missing`] lets through; `RENSETUP_NO_SPLINE_FIT=1` answers it 0 again,
/// counted.
fn spline_fit_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_SPLINE_FIT").is_none())
}

/// Why `SI_animQuickStretchUpdate` cannot run on the update node `node` here: its model is a
/// face or a NURBS curve or surface that works on its stored definition, so the routine
/// would replace the working definition by a copy: `SI_faceCopy (0x08434990)` no crate has
/// on the image; `SI_nurbsCrvCopy (0x084dbbf4)` and `SI_nurbsSrfCopy (0x084dc1a8)` are
/// `si-nurbs`' but not routed for the quick stretch ([`KineOutside`]), so the node would be
/// left with none. Such a node is left as it is.
fn qstretch_copy_missing(m: &Mem, node: u32) -> Option<&'static str> {
    let model = m.u32(node.wrapping_add(8));
    if model == 0 || !matches!(m.i32(model + 0x30), 2 | 9 | 10) || m.u16(model + 0x34) == 0 {
        return None;
    }
    let stored = m.u32(m.u32(model + 0x38).wrapping_add(4 * m.u16(model + 0x36) as u32));
    (m.u32(model + 0x48) == stored).then_some("SI_animQuickStretchUpdate (a face or NURBS model: its working copy is not on the image)")
}

/// `si-camera`'s outside world during the update (`SI_camMat`): the camera's element
/// (`SI_edGetElemFromCamera`, `si-scene`'s), the up-vector relation and its entry
/// (`SI_elemdictFindRelatedElem`, `SI_elemdictGetCurRelation`, `si-dict`'s), its activation
/// curve (`SI_edGetFcvFromRelation`, `si-scene`'s) and the curve's value
/// (`SI_fcurveIntegerValue`, `si-motion`'s); the rest answers 0, counted.
struct CamSide<'a>(&'a RefCell<Calls>);
impl si_mem::Outside for CamSide<'_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let answer = match r.addr {
            0x080f_cfb0 => Some(si_scene::edtools::ed_get_elem_from_camera(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
            0x0813_508c => Some(si_dict::elemrel::find_related_elem(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, Ptr32::new(w(6)), Ptr32::new(w(7))).0),
            0x0813_4db0 => Some(si_dict::elemrel::get_cur_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32).0),
            0x080e_e6f0 => Some(si_scene::world::ed_get_fcv_from_relation(m, Ptr32::new(w(0))).addr()),
            0x0820_0404 => Some(si_motion::fcv::integer_value(m, w(0), f32::from_bits(w(1)), w(2) as i16, w(3) as i16) as u32),
            _ => None,
        };
        let mut c = calls.borrow_mut();
        let e = c.entry(r.name).or_default();
        match answer {
            Some(v) => {
                e.0 += 1;
                si_mem::Ret::eax(v)
            }
            None => {
                e.1 += 1;
                si_mem::Ret::eax(0)
            }
        }
    }
}

/// `si-anim`'s outside world when the update blends a model's shapes (`shape.c`'s
/// interpolators, finding 0030): the spline's touch (`SI_splineTouch`) `si-spline`'s; the
/// model's SHAPES element (`SI_edGetElemFromShape`, `si-scene`'s); the element by index
/// (`SI_elemdictGetElem`, `si-dict`'s); the weight curves (`SI_edFindFcv`,
/// `SI_edFindNextFcv`) and their values (`SI_fcurveValue`) `si-motion`'s; a NURBS curve's
/// point move (`SI_nurbsCrvMovePoint`) `si-nurbs`'s image form; the rest as the envelope's
/// table answers it ([`EnvHost`]: the patch's surface, the polygons' normals, the copies,
/// a NURBS surface's point move, …), else 0, counted (among them the knots after the blend,
/// `SI_knotsCrvReparametrization` and `SI_knotsSrfReparametrization`, which no crate has on
/// the image).
struct AnimSide<'a>(&'a RefCell<Calls>);
impl si_anim::outside::Outside for AnimSide<'_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_anim::outside::Routine, a: &[u32]) -> u32 {
        use si_anim::outside as ao;
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let answer = if r.addr == ao::SI_SPLINE_TOUCH.addr {
            si_spline::spline_image::spline_touch(m, &mut Quiet(calls, "callees of SI_splineTouch"), Ptr32::new(w(0)));
            Some(0)
        } else if r.addr == ao::SI_ED_GET_ELEM_FROM_SHAPE.addr {
            Some(si_scene::edtools::ed_get_elem_from_shape(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0)
        } else if r.addr == ao::SI_ELEMDICT_GET_ELEM.addr {
            Some(si_dict::elemdict::get_elem(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32).0)
        } else if r.addr == ao::SI_ED_FIND_FCV.addr {
            Some(si_motion::ed::find_fcv(m, w(0), w(1) as i32, w(2) as i32))
        } else if r.addr == ao::SI_ED_FIND_NEXT_FCV.addr {
            Some(si_motion::ed::find_next_fcv(m, w(0), w(1) as i32, w(2) as i32))
        } else if r.addr == ao::SI_GET_EXPR_FCURVE_USED.addr && exprfcv_on() {
            Some(si_expr::image::misc::get_expr_fcurve_used(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2), Ptr32::new(w(3)), Ptr32::new(w(4)), w(5) as i32))
        } else if r.addr == ao::SI_NURBS_CRV_MOVE_POINT.addr {
            // x, y, z: three doubles, two words each (low word first)
            let d = |k: usize| f64::from_bits(w(k) as u64 | (w(k + 1) as u64) << 32);
            Some(si_nurbs::image::crv_move_point(m, w(0), w(1) as i32, d(2), d(4), d(6)) as u32)
        } else if r.addr == ao::SI_KNOTS_CRV_REPARAMETRIZATION.addr {
            // routing9: [soft 0x084de590 SI_knotsCrvReparametrization] on the image (a refusal leaves the curve as it was)
            si_nurbs::image::knots_crv_reparametrization(m, w(0)).ok();
            Some(0)
        } else if r.addr == ao::SI_KNOTS_SRF_REPARAMETRIZATION.addr {
            // routing9: [soft 0x084ded40 SI_knotsSrfReparametrization] on the image
            si_nurbs::geom3::knots_srf_reparametrization(m, &mut Quiet(calls, "callees of SI_knotsSrfReparametrization"), w(0));
            Some(0)
        } else {
            si_scene::envside::envelope_call(&mut EnvHost(calls), m, r.addr, a)
        };
        let mut c = calls.borrow_mut();
        let e = c.entry(r.name).or_default();
        match answer {
            Some(v) => {
                e.0 += 1;
                v
            }
            None => {
                e.1 += 1;
                0
            }
        }
    }
    fn call_f(&mut self, m: &mut Mem, r: &'static si_anim::outside::Routine, a: &[u32]) -> si_x87::F80 {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        if r.addr == si_anim::outside::SI_FCURVE_VALUE.addr {
            self.0.borrow_mut().entry(r.name).or_default().0 += 1;
            return si_motion::fcv::value(m, w(0), f32::from_bits(w(1)), w(2) as i16);
        }
        self.0.borrow_mut().entry(r.name).or_default().1 += 1;
        si_x87::F80::ZERO
    }
}

/// `f` with an update host of its own ([`make_host`]): for a routine of `si-motion` that a
/// port of another crate calls back (the readjusting's active time and cluster
/// constraint). The calls it makes are added to `calls`.
fn nested<R>(calls: &RefCell<Calls>, f: impl FnOnce(&mut Trace) -> R) -> R {
    let local = Rc::new(RefCell::new(Calls::new()));
    let mut host = make_host(local.clone());
    let r = f(&mut host);
    drop(host);
    let inner = Rc::try_unwrap(local).map(RefCell::into_inner).unwrap_or_default();
    let mut c = calls.borrow_mut();
    for (k, (yes, no)) in inner {
        let e = c.entry(k).or_default();
        e.0 += yes;
        e.1 += no;
    }
    r
}

/// The dictionary's routines and `SI_activationFcvCheckTime`, by address, as the
/// look-ups of `edtools.c` (`si_scene::edtools`) call them during the update: `si-dict`'s
/// and `si-motion`'s ports.
/// `SI_removeTriangle (0x085efc38)(node, sons)`, `grTriangle.c`: `si-model`'s port on the
/// image (the node's render vertices, triangles and extra texture coordinates freed, its
/// triangle mesh with `SI_mdlRemoveTmesh (0x085f1438)`); its one call out,
/// `SI_glDeleteDispList (0x085f2800)` (`ogl.c`, not ported: GL), made only for a node with a
/// display list, answers 0, counted.
fn remove_triangle(m: &mut Mem, calls: &RefCell<Calls>, a: &[u32]) {
    let w = |k: usize| a.get(k).copied().unwrap_or(0);
    si_model::model_image::triangle::remove_triangle(m, &mut Quiet(calls, "callees of SI_removeTriangle"), Ptr32::new(w(0)), w(1) as i16);
}

/// The outside world of `SI_waveTransform` (si-scene's `wave_sys`, `wave_update`): the
/// wave's element (`SI_elemdictGetElemFromData`, si-dict's), its two curves
/// (`SI_edFindFcv`, si-motion's) and their values (`SI_fcurveValue`, si-motion's), the
/// mesh's normals (si-model's), the patch's surface and the spline's touch (si-spline's).
/// `si_waveGeomUpdate (0x084c81d8)`, which moves a mesh's, patch's or NURBS surface's points,
/// is si-scene's port (`wave_geom`; `RENSETUP_NO_WAVE_GEOM=1` answers it 0 again, counted),
/// its calls out (the model's element, the points read and written, the normals, the
/// copies and disposals) the ports `si_scene::envside::envelope_call` names.
struct WaveSide<'a>(&'a RefCell<Calls>);
impl si_scene::edtools::ByAddr for WaveSide<'_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
        use si_scene::{edtools as ed, wave_sys as ws, wave_update as wu};
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let (name, answer) = match f {
            ed::SI_ELEMDICT_GET_ELEM_FROM_DATA => (
                "SI_elemdictGetElemFromData",
                Some(si_dict::elemmisc::get_elem_from_data(m, &mut Quiet(calls, "callees of SI_elemdictGetElemFromData"), Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)), Ptr32::new(w(3))).0),
            ),
            wu::SI_ED_FIND_FCV => ("SI_edFindFcv", Some(si_motion::ed::find_fcv(m, w(0), w(1) as i32, w(2) as i32))),
            ws::SI_PATCH_COMPUTE_SURFACE => {
                si_spline::patch_image::patch_compute_surface(m, Ptr32::new(w(0)), w(1) as i16);
                ("SI_patchComputeSurface", Some(0))
            }
            ws::SI_MESH_COMPUTES_POLYGON_NORMAL => {
                si_model::mesh_image::rebuild::computes_polygon_normal(m, w(0), w(1) as i32);
                ("SI_meshComputesPolygonNormal", Some(0))
            }
            ws::SI_MESH_COMPUTES_VERTEX_NORMAL => {
                si_model::mesh_image::rebuild::computes_vertex_normal(m, w(0), w(1) as i32);
                ("SI_meshComputesVertexNormal", Some(0))
            }
            ws::SI_SPLINE_TOUCH => {
                si_spline::spline_image::spline_touch(m, &mut Quiet(calls, "callees of SI_splineTouch"), Ptr32::new(w(0)));
                ("SI_splineTouch", Some(0))
            }
            ws::SI_WAVE_GEOM_UPDATE if wave_geom_on() => {
                si_scene::wave_geom::wave_geom_update(&mut WaveSide(calls), m, w(0), w(1), f32::from_bits(w(2)), w(3) as i16);
                ("si_waveGeomUpdate", Some(0))
            }
            ws::SI_WAVE_GEOM_UPDATE => ("si_waveGeomUpdate (RENSETUP_NO_WAVE_GEOM)", None),
            _ => match si_scene::envside::envelope_call(&mut EnvHost(calls), m, f, a) {
                Some(v) => ("callees of si_waveGeomUpdate", Some(v)),
                None => ("(a callee of SI_waveTransform without an answer)", None),
            },
        };
        let mut c = calls.borrow_mut();
        let e = c.entry(name).or_default();
        match answer {
            Some(v) => {
                e.0 += 1;
                v
            }
            None => {
                e.1 += 1;
                0
            }
        }
    }
}
impl si_scene::wave_update::WaveOutside for WaveSide<'_> {
    fn call_f80(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> si_x87::F80 {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let mut c = self.0.borrow_mut();
        if f == si_scene::wave_update::SI_FCURVE_VALUE {
            c.entry("SI_fcurveValue").or_default().0 += 1;
            drop(c);
            return si_motion::fcv::value(m, w(0), f32::from_bits(w(1)), w(2) as i16);
        }
        c.entry("(a callee of SI_waveTransform without an answer)").or_default().1 += 1;
        si_x87::F80::ZERO
    }
}

/// `RENSETUP_NO_WAVE_GEOM=1` leaves `SI_waveTransform`'s `si_waveGeomUpdate` a hook
/// (answered 0), as before it was ported.
fn wave_geom_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_WAVE_GEOM").is_none())
}

/// `RENSETUP_NO_WAVE_TRANSFORM=1` leaves the readjusting's `SI_waveTransform` a hook
/// (answered 0), as before it was routed.
fn wave_transform_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_WAVE_TRANSFORM").is_none())
}

struct Lookups<'a>(&'a RefCell<Calls>);
impl si_scene::edtools::ByAddr for Lookups<'_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
        use si_scene::edtools as ed;
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        match f {
            ed::SI_ELEMDICT_GET_ELEM => si_dict::elemdict::get_elem(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32).0,
            ed::SI_ELEMDICT_GET_ELEM_FROM_DATA => si_dict::elemmisc::get_elem_from_data(m, &mut Quiet(calls, "callees of SI_elemdictGetElemFromData"), Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)), Ptr32::new(w(3))).0,
            ed::SI_ELEMDICT_FIND_ELEM_BY_DATA => si_dict::elemdict::find_elem_by_data(m, &mut Quiet(calls, "callees of SI_elemdictFindElemByData"), Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2))) as u32,
            ed::SI_ELEMDICT_FIND_RELATED_ELEM => si_dict::elemrel::find_related_elem(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, Ptr32::new(w(6)), Ptr32::new(w(7))).0,
            ed::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM => si_dict::elemrel::find_next_related_elem(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, Ptr32::new(w(6)), Ptr32::new(w(7))).0,
            ed::SI_ELEMDICT_GET_CUR_RELATION => si_dict::elemrel::get_cur_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32).0,
            ed::SI_ACTIVATION_FCV_CHECK_TIME => activation(m, w(0), w(1), w(2)),
            _ => {
                calls.borrow_mut().entry("callees of the look-ups of edtools.c").or_default().1 += 1;
                0
            }
        }
    }
}

/// On (the default): wire4's routes (`SI_symmetryGetElem`, `SI_edElementChangeName`); `RENSETUP_NO_WIRE4=1` leaves them hooks answered 0, counted.
fn wire4_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_WIRE4").is_none())
}

/// The world of `symmetry::symmetry_get_elem`: the model's element is `si-scene`'s port, the relation look-ups
/// `si-dict`'s ([`Lookups`]).
struct SymSide<'a>(&'a RefCell<Calls>);
impl super::outside::Outside for SymSide<'_> {
    fn call(&mut self, m: &mut Mem, f: si_mem::FnPtr, args: &[u32]) -> u32 {
        use si_scene::edtools::ByAddr;
        let w = |k: usize| args.get(k).copied().unwrap_or(0);
        if f == super::outside::SI_ED_GET_ELEM_FROM_MODEL {
            return si_scene::edtools::ed_get_elem_from_model(&mut Lookups(self.0), m, Ptr32::new(w(0)), Ptr32::new(w(1))).0;
        }
        Lookups(self.0).call_addr(m, f.addr(), args)
    }
}

/// `SI_animBooleanUpdate (0x0830a7b0)(node)` on the image with its callees routed ([`animbool::AnimBoolSide`]):
/// for a host of another tool (`motion-evalcheck`) that has its own forward table; `calls` takes the tally.
pub fn anim_boolean_update(m: &mut Mem, calls: &mut Calls, node: u32) {
    let cell = RefCell::new(std::mem::take(calls));
    si_boolean::ops::si_anim_boolean_update(m, &mut animbool::AnimBoolSide(&cell), node);
    *calls = cell.into_inner();
}

/// `SI_activationFcvCheckTime (0x0822aa64)(curve, time, &time out, 1)`: `si-motion`'s port;
/// the time goes to the third argument, the answer is whether the curve says "on".
pub fn activation(m: &mut Mem, curve: u32, time_bits: u32, out: u32) -> u32 {
    let (t, on) = si_motion::update::activation_fcv_check_time(m, curve, f32::from_bits(time_bits));
    if out != 0 {
        m.set_f32(out, t);
    }
    on as u32
}

/// The port another crate has on the image for a host method of `si-motion`, by name;
/// `None` for one that has none.
fn forward(m: &mut Mem, name: &'static str, a: &[Arg], calls: &RefCell<Calls>) -> Option<Ret> {
    let w = |k: usize| a.get(k).map_or(0, word);
    if !setters_on() && SETTERS.contains(&name)
        || !klusters_on() && KLUSTERS.contains(&name)
        || !deform_lookups_on() && DEFORM_LOOKUPS.contains(&name)
        || !shapes_on() && SHAPES.contains(&name)
    {
        return None;
    }
    Some(match name {
        // ---- si-model: SI_modelUpdateSonState (0x084662f8)
        "model_update_son_state" => {
            si_model::model_image::state::update_son_state(m, &mut SonSide(calls), Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16);
            Ret::default()
        }
        // ---- si-model: the setters the update calls and what the constraints read of a
        // node (finding 0015, sixth pass: SI_modelSetGlobalTranslation (0x0846412c),
        // SI_modelSetGlobalRotation (0x08464224), SI_modelSetGlobalScaling (0x084642fc),
        // SI_modelSetRotation (0x08463f54), SI_modelKineStateCompute, SI_modelComputeBounding,
        // SI_modelGetData, SI_elemGetVertex), as re/out/si-model/evalcheck/routing.diff
        // routes them; and SI_modelSetTranslation (0x084640cc), which only si-kine's plane
        // constraint calls: its target is the original's since the envelopes are rebuilt at
        // load and deformed in the update (finding 0029, third pass)
        "model_set_translation" if envelopes_on() => {
            si_model::model_image::setters::set_translation(m, &mut ModelSide(calls), Ptr32::new(w(0)), w(1), w(2) as i32, w(3) as i16);
            Ret::default()
        }
        "model_set_global_translation" => {
            si_model::model_image::setters::set_global_translation(m, &mut ModelSide(calls), Ptr32::new(w(0)), w(1), w(2) as i32, w(3) as i16);
            Ret::default()
        }
        "model_set_global_rotation" => {
            si_model::model_image::setters::set_global_rotation(m, &mut ModelSide(calls), Ptr32::new(w(0)), w(1), w(2) as i32, w(3) as i16);
            Ret::default()
        }
        "model_set_global_scaling" => {
            si_model::model_image::setters::set_global_scaling(m, &mut ModelSide(calls), Ptr32::new(w(0)), w(1), w(2) as i32, w(3) as i16);
            Ret::default()
        }
        "model_set_rotation" => {
            si_model::model_image::setters::set_rotation(m, &mut ModelSide(calls), Ptr32::new(w(0)), w(1), w(2) as i32, w(3) as i16);
            Ret::default()
        }
        "model_kine_state_compute" => {
            si_model::model_image::kine::kine_state_compute(m, Ptr32::new(w(0)), f(a, 1));
            Ret::default()
        }
        "model_compute_bounding" => {
            si_model::model_image::bounds::compute_bounding(m, Ptr32::new(w(0)));
            Ret::default()
        }
        // ---- si-scene: SI_relGetFcvType (0x0810f6c8), the first curve type of an attribute,
        // from the attribute dictionary on the image; the quick stretch asks it for the
        // kinematic curves (LinVel … AngAcc) it fills the model's record from (finding 0032
        // §11: without it QSTRETCH-All_Effects_Demo's record kept the velocities of the
        // positions)
        "rel_get_fcv_type" => Ret::int(si_scene::dbvsfcv::rel_get_fcv_type(m, w(0) as i32)),
        "model_get_data" => Ret::ptr(si_model::model_image::points::get_data(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32)),
        // SI_modelGetVertexPtr (0x0847402c), routed with SI_klusterGetTrueIndex: the
        // cluster constraint reads the point whose number that routine gives
        "model_get_vertex_ptr" => Ret::ptr(si_model::model_image::points::get_vertex_ptr(m, w(0), w(1) as i32, w(2) as i32, w(3))),
        // ---- si-deform: SI_envelopCompUpdate (0x084b6210), envelop.c on the image (finding
        // 0029, third pass); a NURBS skin's working copy, disposal and point moves are
        // si-nurbs' image forms (the envelope's table, `si_scene::envside::envelope_call`)
        "envelop_comp_update" if envelopes_on() => {
            si_deform::envelop::envelop_comp_update(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0)));
            Ret::default()
        }
        // the questions `animEnvelopUpdate` asks first when the update optimises the
        // envelopes (`SI_glbAnim` +0x3a, set at start-up): SI_envelopDispDef (0x084bc244),
        // SI_envelopCopyDef (0x084bd6c8), SI_envelopSkelRoot (0x084bc124), SI_envSkelSetFlag
        // (0x084bd9e4), SI_envelopNeedUpdate (0x084bd900), and the scene's SI_envSkelResetFlag
        // (0x084bdbe0): si-deform's ports
        "envelop_disp_def" if envelopes_on() => {
            si_deform::envtools::envelop_disp_def(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0)));
            Ret::default()
        }
        "envelop_copy_def" if envelopes_on() => {
            si_deform::envtools::envelop_copy_def(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0)), w(1) as i16);
            Ret::default()
        }
        "envelop_skel_root" if envelopes_on() => Ret::ptr(si_deform::envtools::envelop_skel_root(m, Ptr32::new(w(0))).addr()),
        "env_skel_set_flag" if envelopes_on() => {
            si_deform::envtools::env_skel_set_flag(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16, w(3) as i32, w(4) as i16);
            Ret::default()
        }
        "envelop_need_update" if envelopes_on() => Ret::int(si_deform::envtools::envelop_need_update(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i16) as i32),
        "env_skel_reset_flag" if envelopes_on() => {
            si_deform::envtools::env_skel_reset_flag(m, w(0));
            Ret::default()
        }
        // ---- si-deform: klusters.c on the image (finding 0028)
        "kluster_get_true_index" => Ret::int(si_deform::klusters::kluster_get_true_index(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32)),
        "kluster_count_vertex_ref" => Ret::int(si_deform::klusters::kluster_count_vertex_ref(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0)), w(1) as i32, w(2) as i16)),
        "elem_get_vertex" => Ret::int(si_model::model_image::points::elem_get_vertex(m, &mut ModelSide(calls), w(0), w(1), w(2), w(3) as i32, w(4) as i32, w(5) as i32, w(6), w(7)) as i32),
        // ---- si-dict: the look-ups of elemdict.c, elemmisc.c, elemrel.c, reldict.c
        "elemdict_find_elem_by_data" => Ret::int(si_dict::elemdict::find_elem_by_data(m, &mut Quiet(calls, "callees of SI_elemdictFindElemByData"), Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)))),
        "elemdict_find_son" => Ret::ptr(si_dict::elemmisc::find_son(m, Ptr32::new(w(0)), Ptr32::new(w(1))).0),
        "elemdict_find_next_son" => Ret::ptr(si_dict::elemmisc::find_next_son(m, Ptr32::new(w(0)), Ptr32::new(w(1))).0),
        "elemdict_is_elem_used" => Ret::int(si_dict::elemmisc::is_elem_used(m, Ptr32::new(w(0)), w(1) as i16) as i32),
        "elemdict_get_relation" => Ret::ptr(si_dict::elemrel::get_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32, w(4) as i32).0),
        "reldict_nb_rel_for_deact_count" => Ret::int(si_dict::reldict::nb_rel_for_deact_count(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i32)),
        "reldict_is_rel_active" => Ret::int(si_dict::reldict::is_rel_active(m, Ptr32::new(w(0)), w(1) as i32, w(2) as i16)),
        "reldict_get_rel_updlattr" => {
            si_dict::reldict::get_rel_updlattr(m, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)), Ptr32::new(w(3)));
            Ret::default()
        }
        // ---- si-scene: the look-ups of edtools.c
        "db_chapter2graf_type" => Ret::int(si_scene::edtools::db_chapter2graf_type(&mut Lookups(calls), m, w(0) as i32, w(1) as i32)),
        "ed_is_item_constrained" => Ret::int(si_scene::edtools::ed_is_item_constrained(&mut Lookups(calls), m, w(0), w(1), w(2) as i32, w(3) as i32, w(4) as i16, f32::from_bits(w(5))) as i32),
        "ed_get_elem_from_camera" => Ret::ptr(si_scene::edtools::ed_get_elem_from_camera(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_light" => Ret::ptr(si_scene::edtools::ed_get_elem_from_light(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_cam_from_cam_int" => Ret::int(si_scene::edtools::ed_get_cam_from_cam_int(&mut Lookups(calls), m, w(0) as i32)),
        "ed_get_lig_from_lig_int" => Ret::int(si_scene::edtools::ed_get_lig_from_lig_int(&mut Lookups(calls), m, w(0) as i32)),
        "ed_get_elem_from_shape" => Ret::ptr(si_scene::edtools::ed_get_elem_from_shape(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_lattice" => Ret::ptr(si_scene::edtools::ed_get_elem_from_lattice(&mut Lookups(calls), m, w(0), w(1) as i16, Ptr32::new(w(2))).0),
        // ---- si-scene: SI_waveGetIconModel (0x084c6f00), the wave's icon (wave.rs)
        "wave_get_icon_model" => Ret::ptr(si_scene::wave::wave_get_icon_model(m, w(0))),
        // ---- si-view: SI_findGlData (0x0832ee68)
        "find_gl_data" => Ret::ptr(si_view::graflist::find_gl_data(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32).0),
        // ---- si-spline: spline.c on the image, the path code's three questions
        "spline_length" => {
            let v = si_spline::frenet_image::spline_length(m, &mut Quiet(calls, "callees of SI_splineLength"), Ptr32::new(w(0)));
            Ret::float(si_x87::F80::from_f32(v))
        }
        "spline_distance_to_factor" => Ret::float(si_spline::frenet_image::spline_distance_to_factor(m, Ptr32::new(w(0)), f32::from_bits(w(1)))),
        "spline_value" => {
            si_spline::intersect_image::spline_value(m, Ptr32::new(w(0)), f32::from_bits(w(1)), w(2));
            Ret::default()
        }
        // ---- si-nurbs: nrb_geometry.c on the image, the path code's and the tangency
        // constraint's length of a NURBS curve and parameter at a distance along it
        // (SI_nurbsCurveLength (0x08502070), SI_nurbsDistanceToFactor (0x08502f4c)), which
        // build the length tables of a curve that has none (SI_nurbsCurveUpdateLength
        // (0x08501ed8), the touch SI_nurbsCrvAnimUpdate (0x08501d90) with it; its calls into
        // the animation package answered 0, counted, as the spline's touch's are); a curve the
        // port refuses (`si_nurbs::image::Halt`) is left unanswered, counted
        "nurbs_curve_length" => match si_nurbs::image::nurbs_curve_length(m, &mut Quiet(calls, "callees of SI_nurbsCurveLength"), w(0)) {
            Ok(v) => Ret::float(v),
            Err(_) => return None,
        },
        "nurbs_distance_to_factor" => match si_nurbs::image::nurbs_distance_to_factor(m, &mut Quiet(calls, "callees of SI_nurbsCurveLength"), w(0), dbl(a, 1)) {
            Ok(v) => Ret::float(v),
            Err(_) => return None,
        },
        "nurbs_curve_update_length" => match si_nurbs::image::nurbs_curve_update_length(m, &mut Quiet(calls, "callees of SI_nurbsCurveUpdateLength"), w(0), w(1) as i32, w(2) as i16) {
            Ok(()) => Ret::default(),
            Err(_) => return None,
        },
        // ---- si_symmetryVector (0x08244968) of this crate (`native/symmetry_vector.rs`): the symmetry
        // constraint's mirrored component; `RENSETUP_NO_SYMVEC=1` leaves it a hook (answered 0, counted)
        "symmetry_vector" if symvec_on() => {
            super::symmetry_vector::symmetry_vector(m, w(0), w(1), w(2) as i32, w(3), w(4) as i16, w(5));
            Ret::default()
        }
        // ---- wire4: SI_symmetryGetElem (0x082434a0) of this crate (`native/symmetry.rs`), the model a model is
        // symmetric to (the keyframe editing's pairs); `RENSETUP_NO_WIRE4=1` leaves it a hook (answered 0, counted)
        "symmetry_get_elem" if wire4_on() => Ret::ptr(super::symmetry::symmetry_get_elem(&mut SymSide(calls), m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2), w(3) as i16, w(4) as i16).0),
        // ---- wire4: si-scene's edtools.c / edfirst.c look-ups the key editing, the path and the reset routines ask
        // (edit paths: no sample scene's first frame reaches them); `RENSETUP_NO_WIRE4=1` leaves them hooks answered 0
        "ed_get_elem_from_material" if wire4_on() => Ret::ptr(si_scene::edtools::ed_get_elem_from_material(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_texture2d" if wire4_on() => Ret::ptr(si_scene::edtools::ed_get_elem_from_texture2d(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_texture3d" if wire4_on() => Ret::ptr(si_scene::edtools::ed_get_elem_from_texture3d(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_scene" if wire4_on() => Ret::ptr(si_scene::edtools::ed_get_elem_from_scene(&mut Lookups(calls), m, Ptr32::new(w(0))).0),
        "ed_get_elem_from_wave" if wire4_on() => Ret::ptr(si_scene::edtools::ed_get_elem_from_wave(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_cluster" if wire4_on() => Ret::ptr(si_scene::edtools::ed_get_elem_from_cluster(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "ed_get_elem_from_anim_file" if wire4_on() => Ret::ptr(si_scene::edfirst::ed_get_elem_from_anim_file(&mut Lookups(calls), m, w(0), Ptr32::new(w(1))).0),
        "db_graf_type2chapter" if wire4_on() => Ret::int(si_scene::edfirst::db_graf_type2chapter(m, w(0) as i32)),
        "ed_element_set_constraint" if wire4_on() => Ret::int(si_scene::edfirst::ed_element_set_constraint(&mut Lookups(calls), m, w(0), w(1) as i32, w(2), w(3) as i32, w(4) as i16, w(5)) as i32),
        "ed_model_relax_fitting" if wire4_on() => {
            si_scene::edfirst::ed_model_relax_fitting(&mut Lookups(calls), m, w(0), w(1) as i16, w(2) as i16);
            Ret::default()
        }
        // ---- wire1: si-render's tex_leftovers on the image (SI_stripPicExtension 0x082b0538, SI_updTxt2dEffects 0x082b289c,
        // SI_setAreaRect / SI_setAreaDisk, SI_texture2DSetDirty 0x082a8fc0 with its look-ups as [`TexSide`] answers them)
        // and si-expr's SI_modelPropagateMatteParam (0x080f3c00); RENSETUP_NO_PARAMS=1 leaves them hooks answered 0
        "strip_pic_extension" if params_on() => Ret::ptr(si_render::tex_leftovers::strip_pic_extension(m, w(0))),
        "upd_txt2d_effects" if params_on() => {
            si_render::tex_leftovers::upd_txt2d_effects(m, w(0));
            Ret::default()
        }
        "set_area_rect" if params_on() => {
            si_render::tex_leftovers::set_area_rect(m, w(0));
            Ret::default()
        }
        "set_area_disk" if params_on() => {
            si_render::tex_leftovers::set_area_disk(m, w(0));
            Ret::default()
        }
        "texture2d_set_dirty" if params_on() => {
            si_render::tex_leftovers::texture2d_set_dirty(m, &mut TexSide(calls), w(0));
            Ret::default()
        }
        "model_propagate_matte_param" if params_on() => {
            si_expr::image::params::model_propagate_matte_param(m, w(0), f32::from_bits(w(1)));
            Ret::default()
        }
        // ---- si-render: SI_updTxt3dTurb (0x082b8d20), t3d_fct.c on the image
        "upd_txt3d_turb" => {
            si_render::t3d_fct_image::upd_txt3d_turb(m, w(0), &mut si_render::tt::TrigLib);
            Ret::default()
        }
        // ---- si-expr: SI_animExpressionEvaluate (0x082492b0), si-expr's image form
        "anim_expression_evaluate" => {
            let _ = si_expr::image::anim_expression_evaluate(m, &mut ExprOutside(calls), Ptr32::new(w(0)), f(a, 1));
            Ret::default()
        }
        // si-expr's SI_modelSetHiddenParam (0x080f3c44, edparams.c): a model's visibility,
        // with the branch its sons', their calls out as [`ExprOutside`] answers them
        "model_set_hidden_param" => {
            si_expr::image::elem::model_set_hidden_param(m, &mut ExprOutside(calls), w(0), w(1), w(2), w(3) as i32, w(4) as i16);
            Ret::default()
        }
        // ---- si-model: SI_modelReadjustDefinition (0x0846e340), on the image since finding
        // 0015's eighth pass: the working definition rebuilt from the stored one and what
        // deforms it (the lattices si-deform's, the quick stretch si-kine's), its calls out
        // as [`ReadjustSide`] answers them
        "model_readjust_definition" if readjust_on() => {
            si_model::model_image::readjust::readjust_definition(m, &mut ReadjustSide(calls), Ptr32::new(w(0)));
            Ret::default()
        }
        // ---- si-model: SI_revolutionUpdate (0x08196384) and SI_zipUpdate (0x081a3c34), on the
        // image (finding 0015, last sections), their calls out as [`RevZipSide`] answers them
        "revolution_update" if revolution_zip_on() => {
            si_model::model_image::tree::revolution_update(m, &mut RevZipSide(calls), w(0));
            Ret::default()
        }
        "zip_update" if revolution_zip_on() => {
            si_model::model_image::zip::zip_update(m, &mut RevZipSide(calls), w(0));
            Ret::default()
        }
        // ---- si-camera: SI_camMat (0x080d62d4), the update's step for relation 1117
        // (finding 0032), its calls out as [`CamSide`] answers them
        "cam_mat" if camera_on() => {
            si_camera::mat::cam_mat(m, &mut CamSide(calls), w(0), w(1));
            Ret::default()
        }
        // ---- what deforms a model, as the update asks it (`undeformed`, the lattice and
        // fitting interpolations, the shapes' touch): si-deform's SI_modelLattice
        // (0x0843ff1c), SI_latticeInterpolate (0x0843f518), SI_fittingUpdateMatrix
        // (0x0843a264) and SI_pfittingUpdateMatrix (0x0843a484); si-model's
        // SI_modelSplineFitting (0x08472b44), SI_modelPatchFitting (0x08472c3c),
        // SI_modelTouchDefinition (0x0846d530, its calls out as [`ReadjustSide`] answers
        // them) and SI_meshComputesPolygonNormal (0x084599f4); si-kine's
        // SI_modelHasConstrainedClusters (0x081f5250, its calls out as [`KineOutside`])
        "get_expr_fcurve_used" if exprfcv_on() => Ret::ptr(si_expr::image::misc::get_expr_fcurve_used(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2), Ptr32::new(w(3)), Ptr32::new(w(4)), w(5) as i32)),
        "model_lattice" => Ret::ptr(si_deform::lattice::model_lattice(m, Ptr32::new(w(0)), Ptr32::new(w(1))).addr()),
        "model_spline_fitting" => Ret::ptr(si_model::model_image::defs::spline_fitting(m, Ptr32::new(w(0)), w(1))),
        "model_patch_fitting" => Ret::ptr(si_model::model_image::defs::patch_fitting(m, Ptr32::new(w(0)), w(1))),
        // si-model's SI_modelWave (0x084c759c, wave_sys.c): the model's wave list, and
        // whether a branch's wave is in it
        "model_wave" => Ret::ptr(si_model::model_image::readjust::model_wave(m, Ptr32::new(w(0)), w(1))),
        "lattice_interpolate" => {
            si_deform::lattice::lattice_interpolate(m, Ptr32::new(w(0)), f(a, 1), Ptr32::new(w(2)), Ptr32::new(w(3)));
            Ret::default()
        }
        "fitting_update_matrix" => {
            si_deform::fitting::fitting_update_matrix(m, Ptr32::new(w(0)));
            Ret::default()
        }
        "pfitting_update_matrix" => {
            si_deform::fitting::pfitting_update_matrix(m, Ptr32::new(w(0)));
            Ret::default()
        }
        "model_touch_definition" => {
            si_model::model_image::touch::touch_definition(m, &mut ReadjustSide(calls), Ptr32::new(w(0)));
            Ret::default()
        }
        "mesh_computes_polygon_normal" => {
            si_model::mesh_image::rebuild::computes_polygon_normal(m, w(0), w(1) as i32);
            Ret::default()
        }
        "model_has_constrained_clusters" => Ret::int(si_kine::cnsedit::model_has_constrained_clusters(m, &mut KineOutside(calls), Ptr32::new(w(0))) as i16 as i32),
        // ---- si-kine: SI_quickStretchActive (0x085a23c0), whether quick stretch deforms
        // the model, its two look-ups as [`KineOutside`] answers them
        "quick_stretch_active" => Ret::int(si_kine::image::qstretch::quick_stretch_active(m, &mut KineOutside(calls), w(0)) as i16 as i32),
        // si-deform's SI_postAnimTouch (0x08153618), the update's touch after a lattice or a
        // wave moved, its calls out as [`DeformSide`] answers them
        "post_anim_touch" => {
            si_deform::deformation::post_anim_touch(m, &mut si_deform::routed::Routed::new(&mut DeformSide(calls)), Ptr32::new(w(0)), w(1) as i16);
            Ret::default()
        }
        // ---- si-anim: shape.c's interpolators on the image (finding 0030), the update's
        // blend of a model's shapes, their calls out as [`AnimSide`] answers them: linear
        // between two shapes (from, to, fraction, result), si_meshInterpolate (0x0823dae0),
        // si_patchInterpolate (0x0823dc4c), si_faceInterpolate (0x0823dd04),
        // si_splineInterpolate (0x0823e340); cardinal through four,
        // si_meshCardinalInterpolate (0x0823e430), si_patchCardinalInterpolate (0x0823ea48),
        // si_faceCardinalInterpolate (0x0823ed1c), si_splineCardinalInterpolate (0x0823ef94);
        // weighted over all (model, update node, time), si_meshWeightInterpolate
        // (0x0823fe04), si_patchWeightInterpolate (0x0824037c), si_faceWeightInterpolate
        // (0x08240758), si_splineWeightInterpolate (0x08240d14); for NURBS curves (a face of
        // NURBS curves among the face's), their points moved by si-nurbs's image form of
        // SI_nurbsCrvMovePoint (0x084e9eec), si_nurbsCrvInterpolate (0x0823dfc0),
        // si_nurbsCrvCardinalInterpolate (0x0823f0cc), si_nurbsCrvWeightInterpolate
        // (0x082411c8), and for NURBS surfaces, their points moved by si-nurbs's image form of
        // SI_nurbsSrfMovePoint (0x084e9f2c), si_nurbsSrfInterpolate (0x0823e180),
        // si_nurbsSrfCardinalInterpolate (0x0823f77c), si_nurbsSrfWeightInterpolate
        // (0x082417fc); the knots after the blend (`SI_knotsCrvReparametrization
        // (0x084de590)`, `SI_knotsSrfReparametrization`) not on the image, answered 0,
        // counted. The update does not read what they answer.
        "mesh_interpolate" => {
            si_anim::shape::mesh_interpolate(m, pt(a, 0), pt(a, 1), f(a, 2), pt(a, 3));
            Ret::default()
        }
        "patch_interpolate" => {
            si_anim::shape::patch_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), f(a, 2), pt(a, 3));
            Ret::default()
        }
        "face_interpolate" => {
            si_anim::shape::face_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), f(a, 2), pt(a, 3));
            Ret::default()
        }
        "spline_interpolate" => {
            si_anim::shape::spline_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), f(a, 2), pt(a, 3));
            Ret::default()
        }
        "mesh_cardinal_interpolate" => {
            si_anim::shape::mesh_cardinal_interpolate(m, pt(a, 0), pt(a, 1), pt(a, 2), pt(a, 3), f(a, 4), pt(a, 5));
            Ret::default()
        }
        "patch_cardinal_interpolate" => {
            si_anim::shape::patch_cardinal_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), pt(a, 2), pt(a, 3), f(a, 4), pt(a, 5));
            Ret::default()
        }
        "face_cardinal_interpolate" => {
            si_anim::shape::face_cardinal_interpolate(m, pt(a, 0), pt(a, 1), pt(a, 2), pt(a, 3), f(a, 4), pt(a, 5));
            Ret::default()
        }
        "spline_cardinal_interpolate" => {
            si_anim::shape::spline_cardinal_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), pt(a, 2), pt(a, 3), f(a, 4), pt(a, 5));
            Ret::default()
        }
        "mesh_weight_interpolate" => {
            si_anim::shape::mesh_weight_interpolate(m, &mut AnimSide(calls), pt(a, 0), w(1), f(a, 2));
            Ret::default()
        }
        "patch_weight_interpolate" => {
            si_anim::shape::patch_weight_interpolate(m, &mut AnimSide(calls), pt(a, 0), w(1), f(a, 2));
            Ret::default()
        }
        "face_weight_interpolate" => {
            si_anim::shape::face_weight_interpolate(m, &mut AnimSide(calls), pt(a, 0), w(1), f(a, 2));
            Ret::default()
        }
        "spline_weight_interpolate" => {
            si_anim::shape::spline_weight_interpolate(m, &mut AnimSide(calls), pt(a, 0), w(1), f(a, 2));
            Ret::default()
        }
        "nurbs_crv_interpolate" => {
            si_anim::shape::nurbs_crv_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), f(a, 2), pt(a, 3));
            Ret::default()
        }
        "nurbs_crv_cardinal_interpolate" => {
            si_anim::shape::nurbs_crv_cardinal_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), pt(a, 2), pt(a, 3), f(a, 4), pt(a, 5));
            Ret::default()
        }
        "nurbs_crv_weight_interpolate" => {
            si_anim::shape::nurbs_crv_weight_interpolate(m, &mut AnimSide(calls), pt(a, 0), w(1), f(a, 2));
            Ret::default()
        }
        "nurbs_srf_interpolate" => {
            si_anim::shape::nurbs_srf_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), f(a, 2), pt(a, 3));
            Ret::default()
        }
        "nurbs_srf_cardinal_interpolate" => {
            si_anim::shape::nurbs_srf_cardinal_interpolate(m, &mut AnimSide(calls), pt(a, 0), pt(a, 1), pt(a, 2), pt(a, 3), f(a, 4), pt(a, 5));
            Ret::default()
        }
        "nurbs_srf_weight_interpolate" => {
            si_anim::shape::nurbs_srf_weight_interpolate(m, &mut AnimSide(calls), pt(a, 0), w(1), f(a, 2));
            Ret::default()
        }
        // ---- si-nurbs: SI_nurbsCrvMovePoint (0x084e9eec) on the image, the update's own
        // moves of a NURBS curve's points (the shapes' clusters, the constraints)
        "nurbs_crv_move_point" => {
            si_nurbs::image::crv_move_point(m, w(0), w(1) as i32, dbl(a, 2), dbl(a, 3), dbl(a, 4));
            Ret::default()
        }
        // ---- si-nurbs: SI_nurbsSrfMovePoint (0x084e9f2c) on the image, the same for a NURBS
        // surface (x, y, z and the weight), and SI_nurbsSrfTransferPointsKnots (0x084ea07c),
        // the key edit's points and knots handed from one surface to another; their calls
        // out (SI_genTreeDestroy, markDirtyCcrvs, SI_nurbsSrfForceCapping) answer 0, counted
        "nurbs_srf_move_point" => {
            si_nurbs::image::srf_move_point(m, &mut si_scene::envside::NurbsSide(&mut ModelSide(calls)), w(0), w(1) as i32, dbl(a, 2), dbl(a, 3), dbl(a, 4), dbl(a, 5));
            Ret::default()
        }
        "nurbs_srf_transfer_points_knots" => {
            si_nurbs::image::srf_transfer_points_knots(m, &mut si_scene::envside::NurbsSide(&mut ModelSide(calls)), w(0), w(1));
            Ret::default()
        }
        // ---- si-spline: SI_patchComputeSurface (0x0847c1d0, an adapter there), the surface
        // of a patch whose points the shapes' cluster update or a constraint moved
        "patch_compute_surface" => {
            si_spline::patch_image::patch_compute_surface(m, Ptr32::new(w(0)), w(1) as i16);
            Ret::default()
        }
        // ---- si-nurbs: SI_nurbsCrvValue (0x084d3550) on the image, the path code's point
        // on a NURBS curve (three doubles through the first argument); a curve the port
        // refuses (`si_nurbs::image::Halt`) is left unanswered, counted
        "nurbs_crv_value" => match si_nurbs::image::crv_value(m, w(1), dbl(a, 2), w(3) as i32) {
            Ok(v) => {
                for (k, c) in v.into_iter().enumerate() {
                    m.set_f64(w(0).wrapping_add(8 * k as u32), c);
                }
                Ret::default()
            }
            Err(_) => return None,
        },
        // ---- si-kine: the inverse kinematics, the joints' limits and the constraints'
        // directions on the image (finding 0022, third pass), with si-kine's own calls out
        // handed back here ([`KineOutside`])
        name if kine(name) => return kine_forward(m, name, a, calls),
        // ---- si-dynamics: SI_dynaAlgoTypeSet (0x084a0e14) on the image (routing6): the parameter
        // update's switch of a model's dynamics algorithm
        // ---- si-boolean: SI_animBooleanUpdate (0x0830a7b0) on the image (routing7), the callees as
        // [`animbool::AnimBoolSide`] routes them
        "anim_boolean_update" if animbool::routing7_on() => {
            si_boolean::ops::si_anim_boolean_update(m, &mut animbool::AnimBoolSide(calls), w(0));
            Ret::default()
        }
        "dyna_algo_type_set" if routing6_on() => {
            si_dynamics::settings_rec::si_dyna_algo_type_set(m, w(0), w(1) as i16);
            Ret::default()
        }
        // ---- si-dynamics: SI_dynachnGeometryUpdate (0x084a0b30) on the image (routeF), the scene
        // update's refresh of the mass properties of a dynamics chain; its callees as the dynamic
        // chains' world answers them (dynachains::DynaSide, with si-dynamics' routed ports)
        "dynachn_geometry_update" if dynachains::on() => {
            let mut base = dynachains::DynaSide(calls);
            let mut routed = si_dynamics::routed::Routed::new(&mut base);
            si_dynamics::settings_dyn::si_dynachn_geometry_update(m, &mut routed, w(0));
            Ret::default()
        }
        _ => return None,
    })
}

/// The host methods handed to `si-model`'s setters and point routines (finding 0023,
/// section 23); `RENSETUP_NO_SETTERS=1` leaves them hooks (answered 0), as before.
const SETTERS: &[&str] = &[
    "model_set_translation",
    "model_set_global_translation",
    "model_set_global_rotation",
    "model_set_global_scaling",
    "model_set_rotation",
    "model_kine_state_compute",
    "model_compute_bounding",
    "model_get_data",
    "elem_get_vertex",
];

/// The host methods of step 3 (finding 0023, section 23): the clusters' true indexes and
/// the vertex pointer; `RENSETUP_NO_KLUSTERS=1` leaves them hooks.
const KLUSTERS: &[&str] = &["kluster_get_true_index", "kluster_count_vertex_ref", "model_get_vertex_ptr"];

/// The host methods of what deforms a model (the lattices, the fittings, the touch, the
/// polygons' normals, the constrained clusters, the touch after the animation moved a
/// deformation, the quick stretch's switch); `RENSETUP_NO_DEFORM_LOOKUPS=1` leaves them
/// hooks (answered 0), as before they were routed, and with them the readjusting's
/// `SI_patchTransferPoints` and `SI_removeTriangle` ([`ReadjustSide`], [`ModelSide`]).
const DEFORM_LOOKUPS: &[&str] = &[
    "model_lattice",
    "model_spline_fitting",
    "model_patch_fitting",
    "model_wave",
    "lattice_interpolate",
    "fitting_update_matrix",
    "pfitting_update_matrix",
    "model_touch_definition",
    "mesh_computes_polygon_normal",
    "model_has_constrained_clusters",
    "post_anim_touch",
    "quick_stretch_active",
];

/// The host methods of `shape.c`'s interpolators (si-anim's) and the geometry routines
/// routed with them (`SI_patchComputeSurface`, si-spline's; `SI_nurbsCrvValue`,
/// `SI_nurbsCrvMovePoint` and `SI_nurbsSrfMovePoint`, si-nurbs's); `RENSETUP_NO_SHAPES=1`
/// leaves them hooks (answered
/// 0), as before they were routed.
const SHAPES: &[&str] = &[
    "mesh_interpolate",
    "patch_interpolate",
    "face_interpolate",
    "spline_interpolate",
    "mesh_cardinal_interpolate",
    "patch_cardinal_interpolate",
    "face_cardinal_interpolate",
    "spline_cardinal_interpolate",
    "mesh_weight_interpolate",
    "patch_weight_interpolate",
    "face_weight_interpolate",
    "spline_weight_interpolate",
    "nurbs_crv_interpolate",
    "nurbs_crv_cardinal_interpolate",
    "nurbs_crv_weight_interpolate",
    "nurbs_srf_interpolate",
    "nurbs_srf_cardinal_interpolate",
    "nurbs_srf_weight_interpolate",
    "patch_compute_surface",
    "nurbs_crv_value",
    "nurbs_crv_move_point",
    "nurbs_srf_move_point",
];

fn shapes_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_SHAPES").is_none())
}

fn deform_lookups_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_DEFORM_LOOKUPS").is_none())
}

fn klusters_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_KLUSTERS").is_none())
}

/// `RENSETUP_NO_ENVELOPES=1` leaves the update's `SI_envelopCompUpdate` and
/// `SI_modelSetTranslation` hooks (answered 0), as before finding 0029's third pass.
fn envelopes_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_ENVELOPES").is_none())
}

/// `RENSETUP_NO_READJUST=1` leaves the update's `SI_modelReadjustDefinition` a hook
/// (answered 0), as before it was routed.
fn readjust_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_READJUST").is_none())
}

/// `RENSETUP_NO_ROUTING3=1` leaves `SI_getExprFcurveUsed` (the curves the expressions of an
/// element drive: `si-expr`'s port) a hook answered 0, as before routing3.
fn exprfcv_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_ROUTING3").is_none())
}

/// `RENSETUP_NO_CAMERA=1` leaves the update's `SI_camMat` a hook (answered 0), as before it
/// was routed.
fn camera_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_CAMERA").is_none())
}

fn setters_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_SETTERS").is_none())
}

/// The host methods [`kine_forward`] hands to `si-kine`.
fn kine(name: &str) -> bool {
    matches!(
        name,
        "inv_kin_set_goal"
            | "ske_chain_root"
            | "inv_kin_reset_pref_angle"
            | "inv_kin_set_cns_position"
            | "model_solve_inv_kin"
            | "rotlim_constraint"
            | "poslim_constraint"
            | "constr3pts_map_direction"
            | "tan_constr_map_direction"
            | "nor_constr_map_direction"
            | "tan_constr_create_data"
            | "tan_constr_compute_tan_vector"
            | "tan_vector_from_path"
            | "nor_constr_compute_normal_vec"
            | "anim_model_bound_plane_update"
    )
}

fn f(a: &[Arg], k: usize) -> f32 {
    f32::from_bits(a.get(k).map_or(0, word))
}

/// A `double` argument (any other kind by its 32 bits, as a stack word).
fn dbl(a: &[Arg], k: usize) -> f64 {
    match a.get(k) {
        Some(Arg::D(v)) => f64::from_bits(*v),
        Some(x) => word(x) as f64,
        None => 0.0,
    }
}

fn pt<T>(a: &[Arg], k: usize) -> Ptr32<T> {
    Ptr32::new(a.get(k).map_or(0, word))
}

fn v3(a: &[Arg], k: usize) -> si_types::geom::Vector {
    si_types::geom::Vector { x: f(a, k), y: f(a, k + 1), z: f(a, k + 2) }
}

/// The update's calls into `si-kine`'s ports on the image, by the host method's name (as
/// `re/tools/si-motion/evalcheck`'s `kine::forward` routes them, without its stand-ins).
fn kine_forward(m: &mut Mem, name: &str, a: &[Arg], calls: &RefCell<Calls>) -> Option<Ret> {
    let w = |k: usize| a.get(k).map_or(0, word);
    let node = |k: usize| Ptr32::new(w(k));
    let mut o = KineOutside(calls);
    Some(match name {
        "inv_kin_set_goal" => {
            si_kine::image::invkin::inv_kin_set_goal(m, node(0), v3(a, 1));
            Ret::default()
        }
        "ske_chain_root" => Ret::ptr(si_kine::onimage::ske_chain_root(m, node(0)).addr()),
        "inv_kin_reset_pref_angle" => Ret::int(si_kine::image::invkin::inv_kin_reset_pref_angle(m, node(0), w(1), v3(a, 2)) as i32),
        "inv_kin_set_cns_position" => {
            si_kine::image::invkin::inv_kin_set_cns_position(m, &mut o, node(0), w(1) as i32, v3(a, 2), f(a, 5));
            Ret::default()
        }
        "model_solve_inv_kin" => {
            si_kine::image::invkin::model_solve_inv_kin(m, &mut o, node(0), v3(a, 1), f(a, 4));
            Ret::default()
        }
        "rotlim_constraint" => {
            si_kine::image::limits::rotlim_constraint(m, &mut o, node(0), w(1) as i32);
            Ret::default()
        }
        "poslim_constraint" => {
            si_kine::image::limits::poslim_constraint(m, &mut o, node(0), v3(a, 1), w(4) as i32);
            Ret::default()
        }
        "constr3pts_map_direction" => {
            si_kine::image::mapdir::constr_3pts_map_direction(m, &mut o, node(0), v3(a, 1), v3(a, 4));
            Ret::default()
        }
        "tan_constr_map_direction" => Ret::int(si_kine::image::mapdir::tan_constr_map_direction(m, &mut o, node(0), v3(a, 1)) as i32),
        "nor_constr_map_direction" => Ret::int(si_kine::image::mapdir::nor_constr_map_direction(m, &mut o, node(0), v3(a, 1), v3(a, 4)) as i32),
        "tan_constr_create_data" => Ret::ptr(si_kine::onimage::tan_constr_create_data(m).addr()),
        "tan_constr_compute_tan_vector" => Ret::int(si_kine::tanconstr::tan_constr_compute_tan_vector(m, &mut o, node(0), node(1), Ptr32::new(w(2))) as i16 as i32),
        "tan_vector_from_path" => Ret::int(si_kine::tanconstr::tan_vector_from_path(m, &mut o, Ptr32::new(w(0)), node(1), Ptr32::new(w(2))) as i16 as i32),
        "nor_constr_compute_normal_vec" => Ret::int(si_kine::norconstr::nor_constr_compute_normal_vec(m, &mut o, node(0), node(1), Ptr32::new(w(2))) as i16 as i32),
        "anim_model_bound_plane_update" => Ret::int(si_kine::pivot::anim_model_bound_plane_update(m, &mut o, node(0), node(1), f(a, 2), w(3) as i32, w(4) as i16)),
        _ => return None,
    })
}

/// `si-kine`'s outside world during the update: the routines it calls that another crate
/// has on the image go to that port (through [`forward`] where the update calls the same
/// routine), the rest answer 0 and are counted under the original's name.
struct KineOutside<'a>(&'a RefCell<Calls>);
impl si_kine::outside::Outside for KineOutside<'_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_kine::outside::Routine, a: &[u32]) -> si_kine::outside::Ret {
        use si_kine::outside::Ret as KRet;
        let p = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let via = |m: &mut Mem, name: &'static str, args: &[Arg]| {
            let r = forward(m, name, args, calls);
            let mut t = calls.borrow_mut();
            let e = t.entry(name).or_default();
            if r.is_some() {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
            r
        };
        let i = |k: usize| Arg::I(p(k));
        let answer = match r.name {
            "SI_modelUpdateSonState" => via(m, "model_update_son_state", &[i(0), i(1), i(2)]).map(|_| KRet::int(0)),
            "SI_edGetElemFromModel" => {
                let e = si_scene::edtools::ed_get_elem_from_model(&mut Lookups(calls), m, Ptr32::new(p(0)), Ptr32::new(p(1)));
                Some(KRet::int(e.addr()))
            }
            "SI_elemdictFindRelatedElem" => Some(KRet::int(
                si_dict::elemrel::find_related_elem(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i16, p(3) as i32, p(4) as i32, p(5) as i32, Ptr32::new(p(6)), Ptr32::new(p(7))).addr(),
            )),
            "SI_elemdictGetCurRelation" => Some(KRet::int(si_dict::elemrel::get_cur_relation(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i16, p(3) as i32).addr())),
            // SI_quickStretchActive's
            "SI_elemdictGetRelationFromIds" => Some(KRet::int(si_dict::elemrel::get_relation_from_ids(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i32, p(3) as i32, p(4) as i32, p(5) as i32).addr())),
            // SI_modelHasConstrainedClusters's two look-ups
            "SI_elemdictFindRangeRelatedElem" => Some(KRet::int(
                si_dict::elemrel::find_range_related_elem(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i16, p(3) as i32, p(4) as i32, p(5) as i32, p(6) as i32, Ptr32::new(p(7)), Ptr32::new(p(8))).addr(),
            )),
            "SI_elemdictFindRangeNextRelatedElem" => Some(KRet::int(
                si_dict::elemrel::find_range_next_related_elem(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i16, p(3) as i32, p(4) as i32, p(5) as i32, p(6) as i32, Ptr32::new(p(7)), Ptr32::new(p(8))).addr(),
            )),
            "SI_elemdictFindSerialRelation" => Some(KRet::int(
                si_dict::elemrel::find_serial_relation(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i32, Ptr32::new(p(3)), p(4) as i32, p(5) as i16, p(6) as i16).addr(),
            )),
            "SI_elemdictGetElem" => Some(KRet::int(si_dict::elemdict::get_elem(m, Ptr32::new(p(0)), p(1) as i32, p(2) as i32).addr())),
            "SI_activationFcvCheckTime" => Some(KRet::int(activation(m, p(0), p(1), p(2)))),
            "SI_getExprFcurveUsed" if exprfcv_on() => Some(KRet::int(si_expr::image::misc::get_expr_fcurve_used(m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2), Ptr32::new(p(3)), Ptr32::new(p(4)), p(5) as i32))),
            "SI_constrLocation" => {
                si_motion::update::transfo::constr_location(m, p(0), p(1) as i32, p(2));
                Some(KRet::int(0))
            }
            "SI_symmetryGetElem" if wire4_on() => Some(KRet::int(super::symmetry::symmetry_get_elem(&mut SymSide(calls), m, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2), p(3) as i16, p(4) as i16).0)),
            "SI_edIsItemConstrained" => via(m, "ed_is_item_constrained", &[i(0), i(1), i(2), i(3), i(4), Arg::F(p(5))]).map(|r| KRet::int(r.eax)),
            // si-model's setters (SI_modelSetTranslation as [`forward`] routes it)
            "SI_modelSetRotation" => via(m, "model_set_rotation", &[i(0), i(1), i(2), i(3)]).map(|_| KRet::int(0)),
            "SI_modelSetTranslation" => via(m, "model_set_translation", &[i(0), i(1), i(2), i(3)]).map(|_| KRet::int(0)),
            "SI_modelSetGlobalTranslation" => via(m, "model_set_global_translation", &[i(0), i(1), i(2), i(3)]).map(|_| KRet::int(0)),
            "SI_findGlData" => via(m, "find_gl_data", &[i(0), i(1), i(2)]).map(|r| KRet::int(r.eax)),
            "SI_splineLength" => via(m, "spline_length", &[i(0)]).map(|r| KRet::float(r.st0)),
            "SI_splineDistanceToFactor" => via(m, "spline_distance_to_factor", &[i(0), Arg::F(p(1))]).map(|r| KRet::float(r.st0)),
            "SI_splineValue" => via(m, "spline_value", &[i(0), Arg::F(p(1)), i(2)]).map(|_| KRet::int(0)),
            // the tangency constraint's NURBS path (the distance a double in two words)
            "SI_nurbsCurveLength" => via(m, "nurbs_curve_length", &[i(0)]).map(|r| KRet::float(r.st0)),
            "SI_nurbsDistanceToFactor" => via(m, "nurbs_distance_to_factor", &[i(0), Arg::D(p(1) as u64 | (p(2) as u64) << 32)]).map(|r| KRet::float(r.st0)),
            "SI_splineNbSegment" => Some(KRet::int(si_spline::intersect_image::spline_nb_segment(m, Ptr32::new(p(0))) as u32)),
            "SI_splineUpdateLength" => {
                si_spline::frenet_image::spline_update_length(m, &mut Quiet(calls, "callees of SI_splineUpdateLength"), Ptr32::new(p(0)), p(1) as i16, p(2) as i16);
                Some(KRet::int(0))
            }
            "SI_modelTraversePnts" => {
                let mut o = Traverse(calls);
                Some(KRet::int(si_model::model_image::points::traverse_pnts(m, &mut o, Ptr32::new(p(0)), p(1), p(2), p(3) as i16) as i32 as u32))
            }
            // the quick stretch's callees (finding 0032): the curves' values si-motion's, the
            // relation's group si-dict's, the kinematic curves' triplet si-motion's (its
            // SI_relGetFcvType through the update's host), the working copies as the
            // envelope's table has them (a mesh's, a spline's, a patch's)
            "SI_fcurveValue" => Some(KRet::float(si_motion::fcv::value(m, p(0), f32::from_bits(p(1)), p(2) as i16))),
            "SI_reldictGetRelChapGrp" => Some(KRet::int(si_dict::reldict::get_rel_chap_grp(m, Ptr32::new(p(0)), p(1) as i32, Ptr32::new(p(2))) as u32)),
            "SI_getFcvTriplet" => {
                nested(calls, |h| si_motion::utils::get_fcv_triplet(m, h, p(0), p(1), p(2), p(3), p(4) as i32, p(5) as i32));
                Some(KRet::int(0))
            }
            "SI_genTreeDestroy" if gen_tree_on() => {
                si_gen::tree::destroy(m, &mut GenSide(calls), p(0), p(1));
                Some(KRet::int(0))
            }
            "SI_patchCopy" | "SI_meshCopy" | "SI_splineCopy" => si_scene::envside::envelope_call(&mut EnvHost(calls), m, r.addr, a).map(KRet::int),
            // the deformation's (`SI_QuickStretchCalculation`, finding 0032 §11): the box
            // and the points si-model's; its "Unexpected default" message is not printed
            // (this host has no standard error), counted
            "SI_modelComputeBoundingEL" => Some(KRet::int(si_model::model_image::bounds::compute_bounding_el(m, Ptr32::new(p(0)), p(1), p(2), p(3)) as u32)),
            "SI_elemGetNbVertices" => Some(KRet::int(si_model::model_image::points::elem_get_nb_vertices(m, &mut ModelSide(calls), p(0), p(1), p(2) as i32))),
            "SI_elemGetVertex" => Some(KRet::int(si_model::model_image::points::elem_get_vertex(m, &mut ModelSide(calls), p(0), p(1), p(2), p(3) as i32, p(4) as i32, p(5) as i32, p(6), p(7)))),
            "SI_elemSetVertex" => Some(KRet::int(si_model::model_image::points::elem_set_vertex(m, &mut ModelSide(calls), p(0), p(1), p(2), p(3) as i32, p(4) as i32, p(5) as i32, p(6)))),
            "fprintf" => {
                calls.borrow_mut().entry("fprintf (quick stretch's unexpected default, not printed)").or_default().1 += 1;
                Some(KRet::int(0))
            }
            _ => None,
        };
        answer.unwrap_or_else(|| {
            calls.borrow_mut().entry(r.name).or_default().1 += 1;
            KRet::default()
        })
    }
}

/// `SI_modelTraversePnts`'s world: its callback is a function of `si-kine` (`checkLow`),
/// the rest is answered 0 and counted.
struct Traverse<'a>(&'a RefCell<Calls>);
impl si_model::model_image::Outside for Traverse<'_> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, args: &[u32]) -> u32 {
        if callee.name == si_model::model_image::THROUGH_POINTER {
            if let Some(r) = si_kine::callbacks::call(m, &mut KineOutside(self.0), callee.addr, args) {
                return r;
            }
        }
        self.0.borrow_mut().entry("callees of SI_modelTraversePnts").or_default().1 += 1;
        0
    }
}

/// The host every update of this file runs with: each call out of `si-motion` answered by
/// [`forward`] or by 0, and counted in `c`.
fn make_host(c: Rc<RefCell<Calls>>) -> Trace {
    // RENSETUP_MOTION_TRACE=1: each call out of si-motion on the standard error with the
    // image's fault count before it, to find where an update first faults (a check's aid)
    let trace = std::env::var_os("RENSETUP_MOTION_TRACE").is_some();
    Trace::with(move |m, name, args| {
        if trace {
            eprintln!("motion: {name} {:x?} faults {} first {:x?}", args, m.fault_count(), m.fault());
        }
        let r = forward(m, name, args, &c);
        let mut t = c.borrow_mut();
        let e = t.entry(name).or_default();
        match r {
            Some(r) => {
                e.0 += 1;
                r
            }
            None => {
                e.1 += 1;
                Ret::default()
            }
        }
    })
}

/// `SI_sceneUpdateItems(time, flag, camera, copy the camera)` (si-motion's port,
/// [soft 0x082221a0 SI_sceneUpdateItems] there), every call it makes out of `si-motion`
/// answered by [`forward`] or by 0; the calls are added to `calls`.
pub fn scene_update_items(m: &mut Mem, time: f32, flag: i16, camera: u32, copy_camera: i16, calls: &mut Calls) {
    let shared = Rc::new(RefCell::new(std::mem::take(calls)));
    let mut host = make_host(shared.clone());
    si_motion::update::scene_update_items(m, &mut host, time, flag, camera, copy_camera);
    drop(host);
    *calls = Rc::try_unwrap(shared).map(RefCell::into_inner).unwrap_or_default();
}

/// `SI_modelMove(model)` (si-motion's port, [soft 0x08229a90 SI_modelMove] there), as the motion
/// blur walk of the renderer asks (`isMoving`): whether the model has a curve in its
/// transformation or geometry lists. Calls out are answered as in [`scene_update_items`].
pub fn model_move(m: &mut Mem, model: u32, calls: &mut Calls) -> u32 {
    let shared = Rc::new(RefCell::new(std::mem::take(calls)));
    let mut host = make_host(shared.clone());
    let r = si_motion::update::transfo::model_move(m, &mut host, model);
    drop(host);
    *calls = Rc::try_unwrap(shared).map(RefCell::into_inner).unwrap_or_default();
    r as u32
}

/// `SI_modelConstraintMove(model)` ([soft 0x08229aec SI_modelConstraintMove] in si-motion), the
/// second question of `isMoving`: whether the model is position-constrained.
pub fn model_constraint_move(m: &mut Mem, model: u32, calls: &mut Calls) -> u32 {
    let shared = Rc::new(RefCell::new(std::mem::take(calls)));
    let mut host = make_host(shared.clone());
    let r = si_motion::update::transfo::model_constraint_move(m, &mut host, model);
    drop(host);
    *calls = Rc::try_unwrap(shared).map(RefCell::into_inner).unwrap_or_default();
    r as u32
}

/// `SI_sceneRefresh()` as the load calls it at its end (finding 0018, section 19):
/// si-motion's port ([soft 0x0820f470 SI_sceneRefresh] there), whose `SI_sceneUpdate` is
/// si-motion's port as well ([soft 0x082224b0 SI_sceneUpdate] there: with `refreshObj` 1,
/// `SI_sceneUpdateItems(time, SI_updateOption != 1, null, 0)`), every call out answered as
/// in [`scene_update_items`]; the screen's refresh and the drawing calls answer 0.
pub fn scene_refresh(m: &mut Mem, calls: &mut Calls) {
    let shared = Rc::new(RefCell::new(std::mem::take(calls)));
    let c = shared.clone();
    let mut host = Trace::with(move |m, name, args| {
        if name == "scene_update" {
            c.borrow_mut().entry("scene_update").or_default().0 += 1;
            let mut inner = make_host(c.clone());
            si_motion::update::scene_update(m, &mut inner, f(args, 0));
            return Ret::default();
        }
        c.borrow_mut().entry(name).or_default().1 += 1;
        Ret::default()
    });
    si_motion::play::scene_refresh(m, &mut host);
    drop(host);
    *calls = Rc::try_unwrap(shared).map(RefCell::into_inner).unwrap_or_default();
}

/// The loader's calls for a render command's world (finding 0018, section 19): recorded
/// as `si-scene`'s recorder records them, except `SI_sceneRefresh`, run by
/// [`scene_refresh`] on the image (unless `refresh` is off, `RENSETUP_NO_LOAD_REFRESH=1`
/// in `rensetup-check`). `calls` are the update's calls out of `si-motion`.
#[derive(Default)]
pub struct LoadCalls {
    pub rec: si_scene::calls::Recorder,
    pub refresh: bool,
    pub refreshed: bool,
    pub calls: Calls,
}

impl si_scene::calls::Unported for LoadCalls {
    fn call(&mut self, name: &'static str, args: &[si_scene::calls::Arg]) {
        self.rec.call(name, args);
    }

    fn answer(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> Option<u32> {
        dynachains::loader_answer(m, addr, args)
    }

    fn dynamic_chains_update(&mut self, m: &mut Mem) -> bool {
        if !dynachains::on() {
            return false;
        }
        dynamic_chains_update(m, &mut self.calls);
        true
    }

    fn scene_refresh(&mut self, m: &mut Mem) -> bool {
        if !self.refresh {
            return false;
        }
        scene_refresh(m, &mut self.calls);
        self.refreshed = true;
        true
    }
}

thread_local! {
    /// The C library's generator (`rand`, `srand`): one state for the process, which the
    /// original's batch renderer is for one scene ([`start_process`]).
    static CRAND: RefCell<si_expr::rand::CRand> = RefCell::new(si_expr::rand::CRand::default());
}

/// A new process: the C library's generator as after start-up (`srand(1)`).
pub fn start_process() {
    CRAND.with(|c| *c.borrow_mut() = si_expr::rand::CRand::default());
}

/// routing4's routes out of [`ExprOutside`] (off with `RENSETUP_NO_ROUTING3=1` as routing3's).
fn routing4_on() -> bool {
    std::env::var_os("RENSETUP_NO_ROUTING3").is_none()
}

/// The key a call by address is counted under (one leaked string per address).
fn by_address_key(addr: u32) -> &'static str {
    thread_local! { static KEYS: RefCell<BTreeMap<u32, &'static str>> = RefCell::new(BTreeMap::new()); }
    KEYS.with(|k| *k.borrow_mut().entry(addr).or_insert_with(|| Box::leak(format!("(by address 0x{addr:08x}, from si-expr)").into_boxed_str())))
}

/// The outside world of si-render's `SI_texture2DSetDirty` and its kin (`tex_leftovers`): the element look-ups of
/// `edtools.c` and the related-element walk of the dictionary, the ports [`Lookups`] has; the rest (the screen refresh,
/// the status line, the C library's streams) answered 0, counted.
struct TexSide<'a>(&'a RefCell<Calls>);
impl si_mem::Outside for TexSide<'_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        use si_scene::edtools::ByAddr;
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let mut lookups = Lookups(calls);
        let v = match r.addr {
            0x080f_d3e0 => si_scene::edtools::ed_get_elem_from_texture2d(&mut lookups, m, w(0), Ptr32::new(w(1))).0,
            0x080f_d414 => si_scene::edtools::ed_get_elem_from_texture3d(&mut lookups, m, w(0), Ptr32::new(w(1))).0,
            0x080f_d288 => si_scene::edtools::ed_get_elem_from_material(&mut lookups, m, w(0), Ptr32::new(w(1))).0,
            0x0813_508c | 0x0813_5154 => lookups.call_addr(m, r.addr, a),
            _ => {
                calls.borrow_mut().entry(if r.name.is_empty() { by_address_key(r.addr) } else { r.name }).or_default().1 += 1;
                return si_mem::Ret::NONE;
            }
        };
        si_mem::Ret::int(v as i32)
    }
}

/// wire1's routes out of [`ExprOutside`] (off with `RENSETUP_NO_PARAMS=1`).
fn params_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_PARAMS").is_none())
}

type AttrGetFn = fn(&mut Mem, &mut dyn si_mem::Outside, Ptr32<si_types::dict::ElemIdentity>, Ptr32<si_types::dict::AttGrpIndex>, Ptr32<si_types::dict::AttributeValue>) -> i32;

/// wire1: the routes of [`ExprOutside`] for any outside world of the expression code (a check tool's own, for one).
pub fn wire1_params(m: &mut Mem, o: &mut dyn si_mem::Outside, addr: u32, args: &[u32]) -> Option<si_mem::Ret> {
    if !params_on() {
        return None;
    }
    let r = si_mem::Routine::new("", addr);
    wire1_route(o, m, r, args)
}

fn wire1_route(o: &mut dyn si_mem::Outside, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> Option<si_mem::Ret> {
    ExprOutside::route(o, m, r, args)
}

impl ExprOutside<'_> {
    /// [soft 0x080f9674 SI_materialsAttrGetValue] and the other readers of `si_expr::image::attrget::ROUTINES`,
    /// [soft 0x080f6dd0 SI_modelAttrGetValue], [soft 0x080f0c3c SI_modelGetParam], [soft 0x080f3d1c SI_modelSetParam]
    /// and the `edparams.c` routines of `si_expr::image::params`, called by address.
    fn route(this: &mut dyn si_mem::Outside, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> Option<si_mem::Ret> {
        use si_expr::image::{attrget as ag, modelparam as mp, params as pm};
        let p = |k: usize| args.get(k).copied().unwrap_or(0);
        let i = |k: usize| p(k) as i32;
        let attr: Option<AttrGetFn> = match r.addr {
            0x080f_9674 => Some(ag::materials_attr_get_value),
            0x080f_97e0 => Some(ag::texture2d_attr_get_value),
            0x080f_994c => Some(ag::texture3d_attr_get_value),
            0x080f_9abc => Some(ag::cameras_attr_get_value),
            0x080f_9cb8 => Some(ag::lights_attr_get_value),
            0x080f_9f4c => Some(ag::spot_attr_get_value),
            0x080f_a0c4 => Some(ag::sun_attr_get_value),
            0x080f_a23c => Some(ag::scenes_attr_get_value),
            0x080f_a3a0 => Some(ag::waves_attr_get_value),
            0x080f_abf0 => Some(ag::dummy_attr_get_value),
            0x080f_6dd0 => Some(mp::model_attr_get_value),
            _ => None,
        };
        if let Some(f) = attr {
            return Some(si_mem::Ret::int(f(m, this, Ptr32::new(p(0)), Ptr32::new(p(1)), Ptr32::new(p(2)))));
        }
        let v: u32 = match r.addr {
            0x080f_0c3c => mp::model_get_param(m, this, i(0), p(1), p(2), Ptr32::new(p(3)), i(4)) as i32 as u32,
            0x080f_3d1c => mp::model_set_param(m, this, i(0), p(1), p(2), Ptr32::new(p(3)), i(4)) as u32,
            0x080f_0be8 => pm::get_attr_val_in_boolean(m, p(0)) as u32,
            0x080f_3c00 => {
                pm::model_propagate_matte_param(m, p(0), f32::from_bits(p(1)));
                return Some(si_mem::Ret::NONE);
            }
            0x080f_1930 => pm::wave_get_param(m, i(0), p(1), p(2), p(3)),
            0x080f_4b70 => pm::wave_set_param(m, this, i(0), p(1), p(2), p(3)),
            0x080f_1b64 => pm::cam_get_param(m, i(0), p(1), p(2), p(3)),
            0x080f_4c90 => pm::cam_set_param(m, this, i(0), p(1), p(2), p(3)),
            0x080f_2b7c => pm::material_get_param(m, i(0), p(1), p(2)),
            0x080f_37c4 => pm::text3d_get_param(m, i(0), p(1), p(2)),
            0x080f_6124 => pm::text3d_set_param(m, this, i(0), p(1), p(2)) as u32,
            0x080f_2e30 => pm::text2d_get_param(m, this, i(0), p(1), p(2)),
            0x080f_5be8 => pm::text2d_set_param(m, this, i(0), p(1), p(2)) as u32,
            0x080f_20a8 => pm::light_get_param(m, this, i(0), p(1), p(2), p(3)),
            0x080f_51b8 => pm::light_set_param(m, this, i(0), p(1), p(2), p(3)),
            0x080f_ac08 => pm::scene_get_param(m, this, i(0), p(1), p(2)),
            _ => return None,
        };
        Some(si_mem::Ret::int(v as i32))
    }
}

/// `si-expr`'s outside world during the update: the C library's `rand` and `srand` (one
/// state, [`CRAND`]), `getenv` null, the joints' preferred angle `si-kine`'s port, the sons
/// of an element `si-dict`'s (`SI_edFindSon`, `SI_edFindNextSon`: `SI_elemdictFindSon` and
/// `SI_elemdictFindNextSon` in the current dictionary), the attribute group
/// `General_Element`'s reading routine called by its address (`SI_generalAttrGetValue`,
/// 0x080f6460) si-expr's port, the rest answered 0 and counted under the original's name.
struct ExprOutside<'a>(&'a RefCell<Calls>);
impl si_mem::Outside for ExprOutside<'_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        let p = |k: usize| args.get(k).copied().unwrap_or(0);
        if r.addr == 0x080f_6460 {
            return si_mem::Ret::int(si_expr::image::elem::general_attr_get_value(m, self, Ptr32::new(p(0)), Ptr32::new(p(1)), Ptr32::new(p(2))));
        }
        // routing4: the routines si-expr reaches by address that a port answers on the image
        // (counted under their names like the others; RENSETUP_NO_ROUTING3=1 leaves them 0)
        if routing4_on() {
            let hit = |this: &Self| this.0.borrow_mut().entry(if r.name.is_empty() { by_address_key(r.addr) } else { r.name }).or_default().0 += 1;
            match r.addr {
                0x0846_412c => {
                    si_model::model_image::setters::set_global_translation(m, &mut ModelSide(self.0), Ptr32::new(p(0)), p(1), p(2) as i32, p(3) as i16);
                    hit(self);
                    return si_mem::Ret::NONE;
                }
                0x0846_4224 => {
                    si_model::model_image::setters::set_global_rotation(m, &mut ModelSide(self.0), Ptr32::new(p(0)), p(1), p(2) as i32, p(3) as i16);
                    hit(self);
                    return si_mem::Ret::NONE;
                }
                0x0846_42fc => {
                    si_model::model_image::setters::set_global_scaling(m, &mut ModelSide(self.0), Ptr32::new(p(0)), p(1), p(2) as i32, p(3) as i16);
                    hit(self);
                    return si_mem::Ret::NONE;
                }
                0x0810_f624 => {
                    hit(self);
                    return si_mem::Ret::int(si_scene::dbvsfcv::fcv_get_rel_grp_from_type(m, p(0) as i32));
                }
                0x0845_abf8 => {
                    si_meta::metaball::metaball_touch(m, &mut ExprOutside(self.0), p(0), p(1));
                    hit(self);
                    return si_mem::Ret::NONE;
                }
                // lic1: SI_modelSetParam's question whether the dictionary leaves a metaball's weight writable
                // (SI_metaLicenseCheckForAttribute takes the bit off in the stand-alone renderer): si-dict's ports
                0x0811_fd8c => {
                    let r = si_dict::attdict::get_grp_index_from_fcurve(m, Ptr32::new(p(0)), p(1) as i32, Ptr32::new(p(2)));
                    hit(self);
                    return si_mem::Ret::int(r);
                }
                0x0811_efe0 => {
                    let r = si_dict::attdict::attribute_get_access_mode(m, Ptr32::new(p(0)), Ptr32::new(p(1)), Ptr32::new(p(2)));
                    hit(self);
                    return si_mem::Ret::int(r);
                }
                _ => {}
            }
        }
        // routing6: the expression code's callees whose ports are on the image (si-scene's
        // relation-data disposal and name records, si-anim's curve-type test); counted under their
        // names; RENSETUP_NO_ROUTING6=1 leaves them 0
        if routing6_on() {
            let hit = |this: &Self| this.0.borrow_mut().entry(r.name).or_default().0 += 1;
            match r.addr {
                0x0824_9c74 => {
                    si_scene::world::expr_rel_data_dispose(m, p(0));
                    hit(self);
                    return si_mem::Ret::NONE;
                }
                0x0812_250c => {
                    let n = si_scene::dbnode::list_node_allocate(m, &mut Quiet(self.0, "callees of SI_dbListNodeAllocate"));
                    hit(self);
                    return si_mem::Ret::int(n.addr() as i32);
                }
                0x0812_23dc => {
                    let s = si_scene::dbnode::format_full_name(m, Ptr32::new(p(0)));
                    hit(self);
                    return si_mem::Ret::int(s.addr() as i32);
                }
                0x0820_a654 => {
                    hit(self);
                    return si_mem::Ret::int(si_anim::fcvdial::is_valid_fcv(p(0), p(1) as i32) as i32);
                }
                // routing9: [soft 0x080f5964 SI_materialSetParam] (si-expr's image form); its dirty mark's callee stays counted
                0x080f_5964 => {
                    let done = si_expr::image::elem::material_set_param(m, self, p(0) as i32, p(1), p(2));
                    hit(self);
                    return si_mem::Ret::int(done as i32);
                }
                _ => {}
            }
        }
        // wire1: the attribute-group readers, the model's parameter routines and the edparams.c get/set routines that
        // si-expr's table reaches by address; counted under their names; RENSETUP_NO_PARAMS=1 leaves them 0 as before
        if params_on() {
            if let Some(ret) = wire1_route(self, m, r, args) {
                self.0.borrow_mut().entry(if r.name.is_empty() { by_address_key(r.addr) } else { r.name }).or_default().0 += 1;
                return ret;
            }
        }
        match r.name {
            "rand" => si_mem::Ret::int(CRAND.with(|c| c.borrow_mut().rand())),
            "srand" => {
                CRAND.with(|c| c.borrow_mut().srand(p(0)));
                si_mem::Ret::NONE
            }
            "getenv" | "printf" => si_mem::Ret::NONE,
            "SI_invKinResetPrefAngle" => {
                let v = si_types::geom::Vector { x: f32::from_bits(p(2)), y: f32::from_bits(p(3)), z: f32::from_bits(p(4)) };
                self.0.borrow_mut().entry("SI_invKinResetPrefAngle (from si-expr)").or_default().0 += 1;
                si_mem::Ret::int(si_kine::image::invkin::inv_kin_reset_pref_angle(m, Ptr32::new(p(0)), p(1), v) as i32)
            }
            "SI_edFindSon" => {
                let ed = Ptr32::new(si_motion::glob::cur_dict(m));
                si_mem::Ret::int(si_dict::elemmisc::find_son(m, ed, Ptr32::new(p(0))).0 as i32)
            }
            "SI_edFindNextSon" => {
                let ed = Ptr32::new(si_motion::glob::cur_dict(m));
                si_mem::Ret::int(si_dict::elemmisc::find_next_son(m, ed, Ptr32::new(p(0))).0 as i32)
            }
            name => {
                let key: &'static str = if name.is_empty() { by_address_key(r.addr) } else { name };
                self.0.borrow_mut().entry(key).or_default().1 += 1;
                si_mem::Ret::NONE
            }
        }
    }
}
