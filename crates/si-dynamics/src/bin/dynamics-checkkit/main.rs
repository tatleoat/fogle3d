// SPDX-License-Identifier: MIT
//! `dynamics-checkkit <vector dir>`: si-dynamics held against the vectors the emulation
//! harness wrote for the original (`re/tools/si-dynamics/collisions.spec`, specgen; finding
//! 0011). `EMUCHECK_MUTATE=1` flips one bit of the first value the port returns or changes.
//!
//! Nothing here reproduces a function of the original.

use si_checkkit::{Args, Kit};
use si_dynamics::collisions as c;
use si_dynamics::contact as ct;
use si_dynamics::contact_bbox as cb;
use si_dynamics::invdynamic as iv;
use si_dynamics::invdynamic_fan as ifan;
use si_dynamics::invdynamic_wind as iwind;
use si_dynamics::invdynamic_mesh as imesh;
use si_dynamics::invdynamic_node as inode;
use si_dynamics::dynamic as d;
use si_dynamics::settings as st;
use si_dynamics::constr as cs;
use si_dynamics::playback as pbk;
use si_dynamics::settings_dyn as sdy;
use si_dynamics::settings_geom as sg;
use si_dynamics::settings_rec as sr;
use si_dynamics::particle_build as pb;
use si_dynamics::particle_data as pd;
use si_dynamics::particle_dial as pdl;
use si_dynamics::particle_mf as pm;
use si_dynamics::particle_mn as pn;
use si_dynamics::particle_util as pu;
use si_dynamics::particle_fcrv as pf;
use si_dynamics::particle_init as pi;
use si_dynamics::particle_mrg as pg;
use si_dynamics::wave_dialog as wd;
use si_dynamics::wave_fcrv as wf;
use si_dynamics::wave_init as wi;
use si_dynamics::wave_mn as wn;
use si_dynamics::wave_sys as ws;
use si_dynamics::wave_util as wu;


// the C library's streams of one case (fopen, fscanf, fprintf ...): the model of tools-checkkit
#[path = "../../../../si-tools/src/bin/tools-checkkit/files.rs"]
mod files;

/// The program's `stderr` word: what the port prints to it goes to the case's `stderr`; a stream the file model opened
/// goes to the model (so that a named file is compared like the emulator's `file:PATH` output).
const STDERR_WORD: u32 = 0x0878_0c88;

struct Streams<'a> {
    inner: &'a mut dyn si_mem::Outside,
    err: u32,
}

impl si_mem::Outside for Streams<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        self.inner.call(m, r, a)
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        if stream >= files::HANDLE0 {
            files::write_to(stream, text);
        } else if stream == self.err {
            self.inner.print(2, text);
        } else {
            self.inner.print(stream, text);
        }
    }
}

/// Runs a port that opens and writes files: the files model is set up first (`open`: the routine opens every file itself)
/// and what it wrote goes to `stdout` like the vector generator builds it.
fn with_files<R>(m: &mut si_mem::Mem, o: &mut dyn si_mem::Outside, a: &Args, open: bool, f: impl FnOnce(&mut si_mem::Mem, &mut dyn si_mem::Outside) -> R) -> R {
    if open { files::begin_open(a) } else { files::begin(a) }
    let err = m.u32(STDERR_WORD);
    let mut w = Streams { inner: o, err };
    let r = f(m, &mut w);
    files::emit(w.inner);
    r
}

/// Three floats named `<n>0`, `<n>1`, `<n>2` ... as the spec lists them (by-value vectors).
fn v3(a: &Args, n: &str) -> [f32; 3] {
    [a.f(&format!("{n}x")), a.f(&format!("{n}y")), a.f(&format!("{n}z"))]
}

/// The routines of other files the port reaches through hooks, as Rust the check can run:
/// the matrix copy (source, destination) is the image copy of 64 bytes.
fn route(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> Option<si_mem::Ret> {
    // DYNAMICS_ROUTED=1 (routing5): the callees of this crate that need only the image run as the crate's own ports, so
    // that vectors generated with the REAL callee in the original (no stand-in) hold the wiring against the original
    if std::env::var_os("DYNAMICS_ROUTED").is_some() && matches!(r.addr, 0x0849_5544 | 0x0849_5624 | 0x0849_fe94 | 0x0849_ff5c | 0x084a_0080 | 0x0849_88f4) {
        if let Some(ret) = si_dynamics::routed::Routed::new(&mut si_mem::Trace::new()).route(m, r.addr, a) {
            ROUTED_HITS.with(|c| c.set(c.get() + 1));
            if std::env::var_os("DYNAMICS_ROUTED_LOG").is_some() { eprintln!("ROUTED-HIT {:08x}", r.addr); }
            // DYNAMICS_ROUTED=mut: the mutation switch of the wiring, one bit of the first output word flipped
            if std::env::var("DYNAMICS_ROUTED").is_ok_and(|v| v == "mut") && matches!(r.addr, 0x0849_5544 | 0x0849_5624) {
                let out = *a.last().unwrap();
                m.set_u32(out, m.u32(out) ^ 1);
            }
            return Some(ret);
        }
    }
    match r.name {
        // SI_fkeyCopy ran as the binary has it in the original (the playback specs name no stand-in for it): si_fkeyNew's
        // malloc(0x24), SI_fkeyInitialize's defaults, then fkeyTransfer's ten fields. Only inside the playback entries.
        "SI_fkeyCopy" if NATIVE_FKEY.with(|c| c.get()) => {
            let (src, p) = (a[0], m.malloc(0x24));
            if p != 0 {
                for off in [0u32, 4, 8, 0x18] {
                    m.set_u32(p + off, 0);
                }
                m.set_u16(p + 0xc, 0);
                m.set_u32(p + 0x10, 0);
                m.set_u16(p + 0x14, 0);
                m.set_u16(p + 0x1c, 0);
                m.set_u16(p + 0x1e, 0xffff);
                m.set_u16(p + 0x20, 0xffff);
                for off in [0u32, 4, 8, 0x18] {
                    m.set_f32(p + off, si_x87::fmov(m.f32(src + off)));
                }
                for off in [0xeu32, 0xc, 0x1e, 0x1c, 0x20, 0x14] {
                    m.set_u16(p + off, m.u16(src + off));
                }
            }
            Some(si_mem::Ret::eax(p))
        }
        "SI_matrix3Copy" => {
            m.copy(a[1], a[0], 36);
            Some(si_mem::Ret::eax(0))
        }
        "SI_matrixCopy" => {
            m.copy(a[1], a[0], 64);
            Some(si_mem::Ret::eax(0))
        }
        // the C library's other stream routines the file ports call: nothing is buffered or fails in the model
        "fflush" | "ferror" => Some(si_mem::Ret::eax(0)),
        // the dialog toolkit's answers that are not words the recorded call holds: the f32 / integer in word `item` of the dialog
        "SI_getTexedFloat" => {
            let (m80, se) = si_x87::x(m.f32(a[0] + 4 * a[1])).to_bits();
            let mut b = [0u8; 10];
            b[..8].copy_from_slice(&m80.to_le_bytes());
            b[8..].copy_from_slice(&se.to_le_bytes());
            Some(si_mem::Ret::st0(b))
        }
        "SI_getTexedLong" => Some(si_mem::Ret::eax(m.u32(a[0] + 4 * a[1]))),
        _ => files::route(m, r, a),
    }
}

thread_local! {
    /// Set while a playback entry runs whose original made its function-curve key copies for real.
    static NATIVE_FKEY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Calls answered by the crate's own port in DYNAMICS_ROUTED mode.
    static ROUTED_HITS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The world buffers of the wave dialog checks: the text block `SI_getTexed` hands out (`wtx`) and the slots the texts
    /// of `SI_setTexed` are copied to (`wdout`, 40 bytes per item).
    static DG: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
    /// The world buffer the stand-in of SI_nodesAllocate hands out, and how many calls it has answered.
    static NC: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
}

/// What the original's stand-in did to the image: `SI_nodesAllocate (model, slot, flag, n)` stored the address of the
/// next 0x90 bytes of the case's `ncbuf` in the word `slot`.
fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], answer: u32) -> Result<(), String> {
    // the stand-in of si_waveCreateIconMdl (wave) made the icon it answered the wave's icon (+0x74)
    if r.name == "si_waveCreateIconMdl" && answer != 0 {
        m.set_u32(a[0].wrapping_add(0x74), answer);
    }
    // the stand-in of si_dialogWave (wave, flag): see specgen_particle.py `dialog_wave`; an answer of 0 edited nothing
    if r.name == "si_dialogWave" && answer != 0 {
        let w = a[0];
        let h = m.u32(w + 0x2c).wrapping_mul(0x9E37_79B1) ^ m.u32(w + 0x30) ^ m.u32(w + 0x34).wrapping_mul(31);
        let h = h ^ (h >> 15);
        let (s0, e0) = (m.i32(w + 0x10), m.i32(w + 0x14));
        m.set_u32(w + 0x28, (h >> 3) & 1);
        m.set_u32(w + 0x10, s0.wrapping_add(((h >> 4) & 3) as i32 - 1) as u32);
        m.set_u32(w + 0x14, e0.wrapping_add(((h >> 6) & 3) as i32 - 1) as u32);
    }
    // the stand-ins of the dialog toolkit (specgen_particle.py `set_texed`, `get_texed`)
    if r.name == "SI_setTexed" {
        let (_, out) = DG.with(|c| c.get());
        let mut t = m.cstr(a[2]);
        t.truncate(39);
        t.resize(40, 0);
        m.set_bytes(out.wrapping_add(40 * a[1]), &t);
    }
    if r.name == "SI_getTexed" {
        let (tx, _) = DG.with(|c| c.get());
        m.set_u32(a[2], tx);
    }
    if r.name == "SI_nodesAllocate" {
        let (base, k) = NC.with(|c| c.get());
        m.set_u32(a[1], base.wrapping_add(k * 0x90));
        NC.with(|c| c.set((base, k + 1)));
    }
    Ok(())
}

fn main() {
    Kit::new(&[
        ("resolveLinearSystemRG", |m, _o, a| c::resolve_linear_system_rg(m, a.v("a"), a.s("rows"), a.s("cols"), a.s("n")).into()),
        ("clipt", |m, _o, a| c::clipt(m, a.f("p"), a.f("q"), a.v("t0"), a.v("t1"), a.i("edge"), a.v("e0"), a.v("e1")).into()),
        ("lineBoxLiangBarskyClipping", |m, _o, a| {
            let mut v = [0.0f32; 18];
            for (k, n) in ["a", "b", "s", "e", "hi", "lo"].iter().enumerate() {
                v[3 * k..3 * k + 3].copy_from_slice(&v3(a, n));
            }
            c::line_box_liang_barsky_clipping(m, v, a.v("pout"), a.v("qout"), a.v("z")).into()
        }),
        ("surfaceOfContactAddVertex", |m, _o, a| c::surface_of_contact_add_vertex(m, a.v("vx"), a.v("vy"), a.v("vz"), a.v("model"), a.v("s")).into()),
        ("surfaceOfContactComputeArea", |m, _o, a| c::surface_of_contact_compute_area(m, a.v("s")).to_bits().into()),
        ("checkBsphBsphCollision", |m, _o, a| c::check_bsph_bsph_collision(m, v3(a, "c"), a.f("r1"), a.f("r2"), a.v("pt"), a.v("ang")).into()),
        ("checkBsphBplnCollision", |m, _o, a| c::check_bsph_bpln_collision(m, a.f("cy"), a.f("r"), a.f("h"), a.v("pt"), a.v("ang")).into()),
        ("checkBsphBboxCollision", |m, _o, a| c::check_bsph_bbox_collision(m, v3(a, "c"), a.f("r"), v3(a, "lo"), v3(a, "hi"), a.v("pt"), a.v("ang")).into()),
        ("detectDynaBspherecollision", |m, o, a| c::detect_dyna_bsphere_collision(m, o, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("mdl4"), a.s("addv"), a.v("soc")).into()),
        ("detectDynaBboxcollision", |m, o, a| c::detect_dyna_bbox_collision(m, o, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("mdl4"), a.s("addv"), a.v("soc")).into()),
        ("detectDynaBboxBsphCollision", |m, o, a| c::detect_dyna_bbox_bsph_collision(m, o, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("mdl4"), a.s("addv"), a.v("soc")).into()),
        ("detectDynaNodeCollision", |m, o, a| {
            c::detect_dyna_node_collision(m, o, 0, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("mdl4"), a.v("kind") as i32, a.v("level") as i32, a.v("short") as i16, a.v("soc")).into()
        }),
        ("si_dynaDetectCollision", |m, o, a| c::si_dyna_detect_collision(m, o, a.v("model1"), a.v("model2")).into()),
        ("booleanCollisionTest", |m, o, a| c::boolean_collision_test(m, o, a.v("p1"), a.v("p2"), a.v("p3"), a.i("shape"), a.s("short"), a.v("soc")).into()),
        ("si_dynaFindCollisionState", |m, o, a| c::si_dyna_find_collision_state(m, o, a.v("model1"), a.v("model2"), [a.v("vx"), a.v("vy"), a.v("vz")]).into()),
        ("si_vectorClean", |m, _o, a| d::si_vector_clean(m, a.v("v"), a.i("kind")).into()),
        ("mymodelBranchUpdate", |m, o, a| d::mymodel_branch_update(m, o, a.v("model")).into()),
        ("SI_dynachnPrepareSimulationData", |m, o, a| d::si_dynachn_prepare_simulation_data(m, o, a.v("model")).into()),
        ("dynaEvaluateCollisions", |m, o, a| d::dyna_evaluate_collisions(m, o, a.v("mdl1"), a.v("mdl2"), a.f("dt"), a.v("v1"), a.v("v2")).into()),
        ("dynaSimulationStep", |m, o, a| d::dyna_simulation_step(m, o, a.v("model"), a.f("dt"), a.i("flag") as i16).into()),
        ("SI_dynaSimulationFrame", |m, o, a| d::si_dyna_simulation_frame(m, o, a.v("model"), a.f("t0"), a.f("t1"), a.i("save") as i16).into()),
        ("dynaEvaluateConstraints", |m, o, a| d::dyna_evaluate_constraints(m, o, a.v("mdl1"), a.v("mdl2")).into()),
        ("dynaFastbandInbound", |m, o, a| d::dyna_fastband_inbound(m, o, a.v("model")).into()),
        ("dynaSlowbandInbound", |m, o, a| d::dyna_slowband_inbound(m, o, a.v("model"), a.f("dt"), a.i("flag") as u16).into()),
        ("dynaSlowbandInbound_self", |m, o, a| d::dyna_slowband_inbound(m, o, a.v("model"), a.f("dt"), a.i("flag") as u16).into()),
        ("dynaFastbandOutbound", |m, _o, a| d::dyna_fastband_outbound(m, a.v("model")).into()),
        ("si_modelDynaRel2AbsMatrixUpdate", |m, _o, a| st::si_model_dyna_rel2abs_matrix_update(m, a.v("model")).into()),
        ("si_modelDynaAbs2RelMatrixUpdate", |m, _o, a| st::si_model_dyna_abs2rel_matrix_update(m, a.v("model")).into()),
        ("si_modelDynaJointsUpdate", |m, _o, a| st::si_model_dyna_joints_update(m, a.v("model")).into()),
        ("si_dynaLinearSpeedCompute", |m, _o, a| st::si_dyna_linear_speed_compute(m, a.v("model"), a.f("px"), a.f("py"), a.f("pz"), a.v("out")).into()),
        ("si_dynachnDeltatCompute", |m, _o, a| st::si_dynachn_deltat_compute(m, a.v("vars")).into()),
        ("inertiaTensorElemBoxFactors", |m, _o, a| st::inertia_tensor_elem_box_factors(m, [a.f("x0"), a.f("y0"), a.f("z0")], [a.f("x1"), a.f("y1"), a.f("z1")], [a.v("o6"), a.v("o7"), a.v("o8"), a.v("o9"), a.v("o10"), a.v("o11")]).into()),
        ("inertiaTensorElemBox", |m, _o, a| st::inertia_tensor_elem_box(m, [a.f("x0"), a.f("y0"), a.f("z0")], [a.f("x1"), a.f("y1"), a.f("z1")], a.f("scale"), a.v("out")).into()),
        ("inertiaTensorTranslate", |m, _o, a| st::inertia_tensor_translate(m, a.v("a"), a.f("s"), [a.f("dx"), a.f("dy"), a.f("dz")], a.s("flag"), a.v("out")).into()),
        ("modelBoundingBoxTransfer", |m, _o, a| sg::model_bounding_box_transfer(m, a.v("p1"), a.v("p2")).into()),
        ("hierarchyBoundingCompute", |m, _o, a| sg::hierarchy_bounding_compute(m, a.v("model")).into()),
        ("hierarchyOverallBoundingCompute", |m, _o, a| sg::hierarchy_overall_bounding_compute(m, a.v("model"), a.v("lo"), a.v("hi")).into()),
        ("hierarchyNodeOverallBoundingCompute", |m, _o, a| sg::hierarchy_node_overall_bounding_compute(m, a.v("root"), a.v("node"), a.v("lo"), a.v("hi")).into()),
        ("hierNodeOverallInertiaTensorCompute", |m, _o, a| {
            let o: [u32; 14] = core::array::from_fn(|i| a.v(&format!("o{i}")));
            sg::hier_node_overall_inertia_tensor_compute(m, a.v("root"), a.v("node"), o).into()
        }),
        ("hierarchyOverallInertiaTensorCompute", |m, _o, a| sg::hierarchy_overall_inertia_tensor_compute(m, a.v("model"), a.v("t1"), a.v("t2"), a.v("s1"), a.v("s2")).into()),
        ("SI_dynachnvarSetDefault", |m, _o, a| sr::si_dynachnvar_set_default(m, a.v("v"), a.s("word")).into()),
        ("si_dynajntvarSetDefault", |m, _o, a| sr::si_dynajntvar_set_default(m, a.v("v")).into()),
        ("si_dynajntcteCompute", |m, _o, a| sr::si_dynajntcte_compute(m, a.v("chain"), a.v("body"), a.s("flag")).into()),
        ("SI_dynachncteCompute", |m, _o, a| sr::si_dynachncte_compute(m, a.v("model"), a.s("flag")).into()),
        ("SI_dynamatterCopy", |m, _o, a| sr::si_dynamatter_copy(m, a.v("s"), a.v("d")).into()),
        ("SI_dynajntpropCopy", |m, _o, a| sr::si_dynajntprop_copy(m, a.v("s"), a.v("d")).into()),
        ("SI_dynachnvarCopy", |m, _o, a| sr::si_dynachnvar_copy(m, a.v("s"), a.v("d"), a.s("flag")).into()),
        ("SI_dynaAlgoTypeSet", |m, _o, a| sr::si_dyna_algo_type_set(m, a.v("model"), a.s("flag")).into()),
        ("SI_modelKineStateRecordState", |m, o, a| sdy::si_model_kine_state_record_state(m, o, a.v("model")).into()),
        ("SI_dynajntvarAllocate", |m, _o, _a| sdy::si_dynajntvar_allocate(m).into()),
        ("SI_dynachnvarAllocate", |m, _o, a| sdy::si_dynachnvar_allocate(m, a.s("word")).into()),
        ("dynageometryTransfer", |m, _o, a| sdy::dynageometry_transfer(m, a.v("p1"), a.v("p2")).into()),
        ("SI_modelVolume", |m, o, a| sdy::si_model_volume(m, o, a.v("model"), a.s("flag")).to_bits().into()),
        ("dynaGeometryModelUpdate", |m, o, a| sdy::dyna_geometry_model_update(m, o, a.v("model"), a.v("matter")).into()),
        ("dynaGeometryHierarchyNodeCompute", |m, o, a| sdy::dyna_geometry_hierarchy_node_compute(m, o, a.v("model"), a.v("vec"), a.v("wt")).into()),
        ("dynaGeometryHierarchyCompute", |m, o, a| sdy::dyna_geometry_hierarchy_compute(m, o, a.v("model"), a.v("vec"), a.v("wt")).into()),
        ("SI_dynaGeometryUpdate", |m, o, a| sdy::si_dyna_geometry_update(m, o, a.v("model")).into()),
        ("SI_dynachnGeometryUpdate", |m, o, a| sdy::si_dynachn_geometry_update(m, o, a.v("model")).into()),
        ("SI_dynaLinkRootToGravity", |m, o, a| sdy::si_dyna_link_root_to_gravity(m, o, a.v("model")).into()),
        ("SI_modelDynaInfoCopy", |m, o, a| sdy::si_model_dyna_info_copy(m, o, a.v("p1"), a.v("p2")).into()),
        ("SI_modelDynaInfoDispose", |m, o, a| sdy::si_model_dyna_info_dispose(m, o, a.v("model"), a.s("flag")).into()),
        ("SI_dynaPhyPropsSetNewRoot", |m, o, a| sdy::si_dyna_phy_props_set_new_root(m, o, a.v("model")).into()),
        ("SI_dynaPhyPropsValidateCopyDelete", |m, _o, a| sdy::si_dyna_phy_props_validate_copy_delete(m, a.v("model"), a.i("sel"), a.s("f")).into()),
        ("SI_dynaPhyPropsValidateParent", |m, o, a| sdy::si_dyna_phy_props_validate_parent(m, o, a.v("p1"), a.v("p2")).into()),
        ("SI_conslistFindPriority", |_m, _o, a| (cs::si_conslist_find_priority(a.s("kind") as i16) as i32).into()),
        ("SI_conslistFindPrevConstraint", |m, _o, a| cs::si_conslist_find_prev_constraint(m, a.v("model"), a.s("prio") as i16, a.s("kind") as i16, a.v("obj"), a.s("flag") as i16, a.v("found")).into()),
        ("SI_conslistConstrAllocate", |m, _o, a| cs::si_conslist_constr_allocate(m, a.s("kind") as i16, a.s("prio") as i16, a.v("obj")).into()),
        ("SI_conslistConstrCompleteAllocation", |m, _o, a| { cs::si_conslist_constr_complete_allocation(m, a.v("c")); 0u32.into() }),
        ("SI_conslistAddConstraint", |m, o, a| cs::si_conslist_add_constraint(m, o, a.v("model"), a.v("c"), a.s("flag") as i16).into()),
        ("conslistDisposeOneConstraint", |m, _o, a| { cs::conslist_dispose_one_constraint(m, a.v("c")); 0u32.into() }),
        ("conslistDisposeConstraint", |m, o, a| cs::conslist_dispose_constraint(m, o, a.v("model"), a.s("kind") as i16, a.v("obj")).into()),
        ("SI_conslistDispose", |m, o, a| { cs::si_conslist_dispose(m, o, a.v("model"), a.s("all") as i16, a.s("kind") as i16); 0u32.into() }),
        ("SI_dynaConstraintRelaxFromData", |m, o, a| cs::si_dyna_constraint_relax_from_data(m, o, a.v("model"), a.v("p2")).into()),
        ("SI_modeldynaConstrListClean", |m, _o, a| { cs::si_modeldyna_constr_list_clean(m, a.v("model")); 0u32.into() }),
        ("SI_dynaConstraintGet", |m, o, a| cs::si_dyna_constraint_get(m, o, a.s("kind") as i16, a.s("flag") as i16).into()),
        ("SI_dynaConstraintBounding", |m, _o, a| { cs::si_dyna_constraint_bounding(m, a.v("model"), a.v("lo"), a.v("hi")); 0u32.into() }),
        ("dynaDefConstraintCheckExistence", |m, _o, a| cs::dyna_def_constraint_check_existence(m, a.s("kind") as i16).into()),
        ("si_constraintPositionGet", |m, _o, a| { cs::si_constraint_position_get(m, a.v("p1"), a.v("p2"), a.s("flag") as i16, a.v("out")); 0u32.into() }),
        ("si_constraintOrientationGet", |m, _o, a| { cs::si_constraint_orientation_get(m, a.v("p1"), a.v("p2"), a.s("flag1") as i16, a.s("flag2") as i16, a.s("axis") as u8, a.v("out")); 0u32.into() }),
        ("SI_dynaSimulationCheckExistence", |m, o, a| { pbk::si_dyna_simulation_check_existence(m, o, a.v("model")); 0u32.into() }),
        ("simulationRemoveElemFrames", |m, o, a| { pbk::simulation_remove_elem_frames(m, o, a.v("model"), a.s("pos") as i16, a.s("rot") as i16); 0u32.into() }),
        ("SI_dynaSimulationDispose", |m, o, a| { pbk::si_dyna_simulation_dispose(m, o, a.v("model"), a.s("flag") as i16); 0u32.into() }),
        ("SI_dynaTransitionCrvDispose", |m, o, a| { pbk::si_dyna_transition_crv_dispose(m, o, a.v("model")); 0u32.into() }),
        ("SI_simseginfoRecordTransition", |m, _o, a| { pbk::si_simseginfo_record_transition(m, a.v("model"), a.i("frame"), a.s("state") as i16); 0u32.into() }),
        ("SI_simseginfoAddTransition", |m, _o, a| { pbk::si_simseginfo_add_transition(m, a.v("model"), a.i("frame")); 0u32.into() }),
        ("fcvFindKeyFrame", |m, _o, a| pbk::fcv_find_key_frame(m, a.v("fcv"), a.f("t"), a.v("out")).into()),
        ("SI_simulatedValuesGet", |m, o, a| (pbk::si_simulated_values_get(m, o, a.v("model"), a.i("frame")) as u32).into()),
        ("simulationRawFcrvSaveElemFrame", |m, o, a| {
            NATIVE_FKEY.with(|c| c.set(true));
            pbk::simulation_raw_fcrv_save_elem_frame(m, o, a.v("model"), a.s("pos") as i16, a.s("rot") as i16, a.f("time"));
            NATIVE_FKEY.with(|c| c.set(false));
            0u32.into()
        }),
        ("SI_simulationRawFcrvSaveKeys", |m, o, a| {
            NATIVE_FKEY.with(|c| c.set(true));
            pbk::si_simulation_raw_fcrv_save_keys(m, o, a.v("model"), a.f("time"));
            NATIVE_FKEY.with(|c| c.set(false));
            0u32.into()
        }),
        ("odynamicWriteBinaryChunk", |m, o, a| { si_dynamics::dynamicio::odynamic_write_binary_chunk(m, o, a.v("file"), a.v("model")); 0u32.into() }),
        ("SI_dynachnObstacleBoundingUpdate", |m, _o, a| sr::si_dynachn_obstacle_bounding_update(m, a.v("model")).into()),
        ("SI_dynachnSetChnListPtrs", |m, _o, a| sr::si_dynachn_set_chn_list_ptrs(m, a.v("model")).into()),
        ("SI_hierarchyCheckEmptyPhyProps", |m, _o, a| sr::si_hierarchy_check_empty_phy_props(m, a.v("model"), a.v("out")).into()),
        ("SI_hierarchyCheckPhyPropsConsistancy", |m, _o, a| sr::si_hierarchy_check_phy_props_consistancy(m, a.v("a"), a.v("model"), a.s("s3"), a.s("s4")).into()),
        ("dynaSlowbandInit", |m, _o, a| d::dyna_slowband_init(m, a.v("model")).into()),
        ("dynaSlowbandIntegrate", |m, _o, a| d::dyna_slowband_integrate(m, a.v("model")).into()),
        ("calculeReaction", |m, _o, a| d::calcule_reaction(m, a.v("model"), [a.f("vx"), a.f("vy"), a.f("vz")], a.v("out")).into()),
        ("dynachnGetMatrices", |m, _o, a| d::dynachn_get_matrices(m, a.v("model")).into()),
        ("linePolygonClipping", |m, _o, a| {
            ct::line_polygon_clipping(m, a.v("line"), a.v("soc"), a.v("os"), a.v("oe"), a.v("p1"), a.v("p2")).into()
        }),
        ("si_dynaEvaluateContactStability", |m, _o, a| {
            ct::si_dyna_evaluate_contact_stability(m, a.v("model"), a.v("soc"), a.v("vx"), a.v("vy"), a.v("vz"), a.v("out")).into()
        }),
        ("si_dynaEvaluateSphereSupport", |m, _o, a| ct::si_dyna_evaluate_sphere_support(m, a.v("mdl1"), a.v("mdl2"), a.v("jv"), a.v("ci"), a.v("pa"), a.v("pb")).into()),
        ("si_dynaEvaluateSphereSupport_zero", |m, _o, a| ct::si_dyna_evaluate_sphere_support(m, a.v("mdl1"), a.v("mdl2"), a.v("jv"), a.v("ci"), a.v("pa"), a.v("pb")).into()),
        ("si_dynaEvaluateBboxSupport", |m, _o, a| {
            cb::si_dyna_evaluate_bbox_support(m, a.v("mdl1"), a.v("mdl2"), a.v("jv"), a.v("ci"), [a.v("ax"), a.v("ay"), a.v("az")], [a.v("bx"), a.v("by"), a.v("bz")]).into()
        }),
        ("si_dynaEvaluateBboxSupport_zero", |m, _o, a| {
            cb::si_dyna_evaluate_bbox_support(m, a.v("mdl1"), a.v("mdl2"), a.v("jv"), a.v("ci"), [a.v("ax"), a.v("ay"), a.v("az")], [a.v("bx"), a.v("by"), a.v("bz")]).into()
        }),
        ("invdynaCheckPtFanEffect", |m, _o, a| iv::invdyna_check_pt_fan_effect(m, a.v("model"), a.v("vx"), a.v("vy"), a.v("vz"), a.v("out")).into()),
        ("invdynaGetBboxFaceFanFrce", |m, _o, a| {
            let v = ["x0", "y0", "z0", "x1", "y1", "z1", "x2", "y2", "z2", "x3", "y3", "z3"].map(|n| a.v(n));
            iv::invdyna_get_bbox_face_fan_frce(m, a.v("model"), v, a.v("thr"), a.v("out"), a.v("o2"), a.v("o3"), a.v("depth") as i32);
            ().into()
        }),
        ("invdynaGetMeshWindFrce", |m, o, a| imesh::invdyna_get_mesh_wind_frce(m, o, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("out1"), a.v("out2")).into()),
        ("invdynaGetNodeFanFrce", |m, _o, a| inode::invdyna_get_node_fan_frce(m, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("out1"), a.v("out2"), a.s("flag") as i32).into()),
        ("invdynaGetNodeWindFrce", |m, o, a| inode::invdyna_get_node_wind_frce(m, o, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("out1"), a.v("out2"), a.s("flag") as i32).into()),
        ("si_invdynaGetFanFrce", |m, o, a| inode::si_invdyna_get_fan_frce(m, o, a.v("mdl1"), a.v("mdl2"), a.v("info"), a.v("out1"), a.v("out2")).into()),
        ("si_invdynaGetWindFrce", |m, o, a| inode::si_invdyna_get_wind_frce(m, o, a.v("mdl1"), a.v("mdl2"), a.v("info"), a.v("out1"), a.v("out2")).into()),
        ("invdynaGetBboxWindFrce", |m, _o, a| {
            iwind::invdyna_get_bbox_wind_frce(m, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("out1"), a.v("out2"));
            ().into()
        }),
        ("invdynaGetBboxFanFrce", |m, _o, a| {
            ifan::invdyna_get_bbox_fan_frce(m, a.v("mdl1"), a.v("mdl2"), a.v("mdl3"), a.v("out1"), a.v("out2"));
            ().into()
        }),
        ("si_invdynaGetConstrFrce", |m, o, a| iv::si_invdyna_get_constr_frce(m, o, a.v("mdl1"), a.v("mdl2"), a.v("info"), a.v("out")).into()),
        ("dynaFastbandIntegrate", |m, _o, a| d::dyna_fastband_integrate(m, a.v("model"), a.f("dt")).into()),
        ("resolveCollision", |m, o, a| {
            c::resolve_collision(m, o, a.v("mdl1"), a.v("mdl2"), a.v("jv"), a.v("info"), a.f("p5"), a.f("p6"), a.f("p7")).into()
        }),
        ("si_resolveMultipleCollision", |m, o, a| {
            c::si_resolve_multiple_collision(m, o, a.v("mdl1"), a.v("mdl2"), a.v("jv"), a.v("info"), a.f("p5")).into()
        }),
        ("contactInitialSpeedArrange", |m, o, a| {
            c::contact_initial_speed_arrange(m, o, a.v("mdl1"), a.v("mdl2"), a.v("info")).into()
        }),
        ("model4BoolPrepare", |m, o, a| {
            c::model4_bool_prepare(m, o, a.v("pp"), a.v("model"), a.i("level"), a.v("vec")).into()
        }),
        ("si_particleCrossProduct", |m, _o, a| {
            pm::si_particle_cross_product(m, a.v("a"), a.v("b"), a.v("o"));
            ().into()
        }),
        ("si_particleNormalize", |m, _o, a| pm::si_particle_normalize(m, a.v("v")).into()),
        ("si_particleTransformPoint", |m, _o, a| {
            pm::si_particle_transform_point(m, a.v("mat"), a.v("p"), a.v("o"));
            ().into()
        }),
        ("si_particleMatrixMultiply", |m, _o, a| {
            pm::si_particle_matrix_multiply(m, a.v("a"), a.v("b"), a.v("o"));
            ().into()
        }),
        ("si_particleMakeIdentity", |m, _o, a| {
            pm::si_particle_make_identity(m, a.v("a"));
            ().into()
        }),
        ("si_particleDataInitialize", |m, _o, a| {
            pd::si_particle_data_initialize(m, a.v("pd"));
            ().into()
        }),
        ("si_particleMeshToPoint", |m, _o, a| pd::si_particle_mesh_to_point(m, a.v("mesh"), a.v("pd")).into()),
        ("particleDataInterp", |m, _o, a| pd::particle_data_interp(m, a.v("a"), a.v("b"), a.f("t"), a.v("o")).into()),
        ("si_particleDataSingleTransform", |m, _o, a| pd::si_particle_data_single_transform(m, a.v("mat"), a.v("src"), a.v("dst")).into()),
        ("si_particleJitterScaleMatrix", |m, _o, a| {
            pm::si_particle_jitter_scale_matrix(m, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("amt"), a.v("o"));
            ().into()
        }),
        ("si_particleJitterTranslateMatrix", |m, _o, a| {
            pm::si_particle_jitter_translate_matrix(m, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("amt"), a.v("o"));
            ().into()
        }),
        ("particleAngle", |m, _o, a| pm::particle_angle(m, a.v("a"), a.v("b")).to_f32().to_bits().into()),
        ("si_particleJitterRotateMatrix", |m, _o, a| {
            pm::si_particle_jitter_rotate_matrix(m, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("amt"), a.v("o"));
            ().into()
        }),
        ("si_particleDataJitter", |m, _o, a| {
            pd::si_particle_data_jitter(m, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("pd"), a.v("amt"));
            ().into()
        }),
        ("si_particleTriSample", |m, _o, a| {
            pd::si_particle_tri_sample(m, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("tris"), a.i("uni"), a.v("seed"), a.v("out"));
            ().into()
        }),
        ("si_particlePath2Xfm", |m, _o, a| pd::si_particle_path2_xfm(m, a.v("paths"), a.v("o"), a.v("up")).into()),
        ("particleGetMdl", |m, o, _a| pu::particle_get_mdl(m, o).into()),
        ("si_particleGetMeshes", |m, o, a| pu::si_particle_get_meshes(m, o, a.v("status")).into()),
        ("si_particleCreateAnimation", |m, o, a| {
            pu::si_particle_create_animation(m, o, a.v("status"), a.v("arg"));
            ().into()
        }),
        ("particleTransferGmat", |m, _o, a| {
            pu::particle_transfer_gmat(m, a.v("model"), a.v("out"));
            ().into()
        }),
        ("particleMakeGmat", |m, o, a| {
            pu::particle_make_gmat(m, o, a.v("model"), a.v("block"));
            ().into()
        }),
        ("si_particleAddPreAnimHideKeys", |m, o, a| {
            pf::si_particle_add_pre_anim_hide_keys(m, o, a.v("status"), a.v("model"));
            ().into()
        }),
        ("si_particleAddPostAnimHideKeys", |m, o, a| {
            pf::si_particle_add_post_anim_hide_keys(m, o, a.v("status"), a.v("model"));
            ().into()
        }),
        ("si_particleMakePeriodic", |m, o, a| {
            pf::si_particle_make_periodic(m, o, a.v("status"), a.v("model"));
            ().into()
        }),
        ("si_particleDataAllocate", |m, _o, a| pd::si_particle_data_allocate(m, a.v("pd")).into()),
        ("si_particleDataFree", |m, _o, a| {
            pd::si_particle_data_free(m, a.v("pd"));
            ().into()
        }),
        ("si_particleMeshToTri", |m, o, a| pd::si_particle_mesh_to_tri(m, o, a.v("mesh"), a.v("pd")).into()),
        ("si_particleComputePaths", |m, o, a| pd::si_particle_compute_paths(m, o, a.v("status"), a.v("from"), a.v("to"), a.v("out")).into()),
        ("particleWriteData", |m, o, a| {
            // the case's FILE * argument is the emulator's first handle; what the port writes is compared as `stdout`
            let _ = a;
            pd::particle_write_data(m, o, 1, a.v("pd"));
            ().into()
        }),
        ("SI_initParticle", |m, _o, _a| {
            pi::si_init_particle(m);
            ().into()
        }),
        ("si_particleDuplicateMesh", |m, o, a| {
            // the stand-in of SI_nodesAllocate (the effect below) hands out the world buffer `ncbuf`, 0x90 bytes a call
            NC.with(|c| c.set((a.buffer("ncbuf"), 0)));
            pg::si_particle_duplicate_mesh(m, o, a.i("count"), a.v("src")).into()
        }),
        ("particleCreateHrcAnimation", |m, o, a| {
            pb::particle_create_hrc_animation(m, o, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("status"), a.i("n")).into()
        }),
        ("particleCreateShapeAnimation", |m, o, a| {
            NC.with(|c| c.set((a.buffer("ncbuf"), 0)));
            pb::particle_create_shape_animation(m, o, &mut pm::Lcg48(0x1234_ABCD_330E), a.v("status"), a.i("n")).into()
        }),
        ("SI_getWaveFcurvePointer", |m, o, a| wf::si_get_wave_fcurve_pointer(m, o, a.v("wave"), a.i("id")).into()),
        ("SI_getWaveFcurvePointerPointer", |m, o, a| {
            wf::si_get_wave_fcurve_pointer_pointer(m, o, a.v("wave"), a.i("id"));
            ().into()
        }),
        ("SI_saveKeySingleWaveFcv", |m, o, a| {
            wf::si_save_key_single_wave_fcv(m, o, a.v("wave"), a.i("id"));
            ().into()
        }),
        ("SI_saveKeyWaveScale", |m, o, a| {
            wf::si_save_key_wave_scale(m, o, a.v("wave"));
            ().into()
        }),
        ("SI_saveKeyWaveRotate", |m, o, a| {
            wf::si_save_key_wave_rotate(m, o, a.v("wave"));
            ().into()
        }),
        ("SI_saveKeyWaveTranslate", |m, o, a| {
            wf::si_save_key_wave_translate(m, o, a.v("wave"), a.v("kind"), a.v("short") as i16);
            ().into()
        }),
        ("SI_saveKeyWaveETranslate", |m, o, a| {
            wf::si_save_key_wave_etranslate(m, o, a.v("wave"));
            ().into()
        }),
        ("saveKeyWaveAll", |m, o, a| {
            wf::save_key_wave_all(m, o, a.v("wave"));
            ().into()
        }),
        ("SI_waveInitSpread", |m, o, a| {
            wf::si_wave_init_spread(m, o, a.v("wave"));
            ().into()
        }),
        ("SI_waveInitDecay", |m, o, a| {
            wf::si_wave_init_decay(m, o, a.v("wave"));
            ().into()
        }),
        ("SI_waveDetachMdl", |m, o, a| {
            wu::si_wave_detach_mdl(m, o, a.v("wave"), a.v("model"), a.i("flag"));
            ().into()
        }),
        ("SI_waveChangeProfile", |m, o, _a| {
            wn::si_wave_change_profile(m, o);
            ().into()
        }),
        ("SI_waveCreateItem", |m, o, _a| {
            wn::si_wave_create_item(m, o);
            ().into()
        }),
        ("SI_waveEditItem", |m, o, _a| {
            wn::si_wave_edit_item(m, o);
            ().into()
        }),
        ("SI_waveAttachItem", |m, o, a| {
            wn::si_wave_attach_item(m, o, a.v("branch") as i16);
            ().into()
        }),
        ("waveSetDg", |m, o, a| {
            DG.with(|c| c.set((a.buffer("wtx"), a.buffer("wdout"))));
            wd::wave_set_dg(m, o, a.v("wave"), a.v("dialog"), a.v("elem"));
            ().into()
        }),
        ("waveGetDg", |m, o, a| {
            DG.with(|c| c.set((a.buffer("wtx"), a.buffer("wdout"))));
            wd::wave_get_dg(m, o, a.v("wave"), a.v("dialog"));
            ().into()
        }),
        ("SI_waveFreeze", |m, o, _a| {
            wn::si_wave_freeze(m, o);
            ().into()
        }),
        ("SI_waveDetachItem", |m, o, _a| {
            wn::si_wave_detach_item(m, o);
            ().into()
        }),
        ("SI_particleAnimation", |m, o, _a| {
            let err = m.u32(STDERR_WORD);
            pn::si_particle_animation(m, &mut Streams { inner: o, err });
            ().into()
        }),
        ("SI_particleSetupReset", |m, _o, _a| {
            pdl::si_particle_setup_reset(m);
            ().into()
        }),
        ("particleHandleHideKeys", |m, o, a| {
            pn::particle_handle_hide_keys(m, o, a.v("status"), a.v("model"), a.v("rep") as i32);
            ().into()
        }),
        ("SI_initWave", |m, _o, _a| {
            wi::si_init_wave(m);
            ().into()
        }),
        ("si_waveAllocate", |m, o, a| wi::si_wave_allocate(m, o, a.v("status")).into()),
        ("si_waveInitHorizProf", |m, _o, a| {
            wi::si_wave_init_horiz_prof(m, a.v("wave"));
            ().into()
        }),
        ("si_waveAmpCreate", |m, o, a| wi::si_wave_amp_create(m, o, a.v("graph"), a.v("model")).into()),
        ("waveFreezeMdl", |m, o, a| {
            wu::wave_freeze_mdl(m, o, a.v("wave"), a.v("model"));
            ().into()
        }),
        ("si_waveResizeIcon", |m, _o, a| {
            wu::si_wave_resize_icon(m, a.v("wave"), a.v("v"));
            ().into()
        }),
        ("waveResizeFunctionCurve", |m, _o, a| {
            wu::wave_resize_function_curve(m, a.v("wave"), a.v("fcv"));
            ().into()
        }),
        ("wavePrintGraph", |m, o, a| {
            // the case's FILE * argument is the emulator's first handle; what the port writes is compared as `stdout`
            let _ = a;
            wu::wave_print_graph(m, o, 1, a.v("graph"));
            ().into()
        }),
        ("si_wavePrintWave", |m, o, a| {
            wu::si_wave_print_wave(m, o, 1, a.v("wave"));
            ().into()
        }),
        ("owaveBinaryWrite", |m, o, a| with_files(m, o, a, true, |m, o| ws::owave_binary_write(m, o, a.v("name"))).into()),
        ("SI_owaveBinaryRead", |m, o, a| with_files(m, o, a, true, |m, o| ws::si_owave_binary_read(m, o, a.v("name"))).into()),
        ("SI_particleSetupSave", |m, o, a| with_files(m, o, a, true, |m, o| pdl::si_particle_setup_save(m, o, a.v("name"))).into()),
        ("SI_particleSetupDefault", |m, _o, _a| pdl::si_particle_setup_default(m).into()),
        ("SI_dbwaveAsciiWrite", |m, o, a| with_files(m, o, a, true, |m, o| ws::si_dbwave_ascii_write(m, o, a.v("name"), a.v("wave"))).into()),
        ("SI_dbwaveAsciiRead", |m, o, a| with_files(m, o, a, true, |m, o| ws::si_dbwave_ascii_read(m, o, a.v("name"))).into()),
        ("si_waveReadWave", |m, o, a| {
            with_files(m, o, a, false, |m, o| wu::si_wave_read_wave(m, o, files::HANDLE0, a.v("wave"), a.v("scale"), a.v("rotate"), a.v("translate"))).into()
        }),
        ("SI_getWaveByName", |m, _o, a| ws::si_get_wave_by_name(m, a.v("name")).into()),
        ("SI_waveDispose", |m, o, a| {
            ws::si_wave_dispose(m, o, a.v("slot"));
            ().into()
        }),
        ("SI_modelUnlinkAllWaves", |m, o, a| {
            ws::si_model_unlink_all_waves(m, o, a.v("model"));
            ().into()
        }),
        ("SI_waveAttachByName", |m, o, a| {
            ws::si_wave_attach_by_name(m, o, a.v("wname"), a.v("mname"), a.i("kind") as i16);
            ().into()
        }),
        ("SI_waveDuplicate", |m, o, a| ws::si_wave_duplicate(m, o, a.v("src")).into()),
        ("waveGetModel", |m, _o, a| ws::wave_get_model(m, a.v("w")).into()),
        ("SI_waveGetIconModel", |m, _o, a| ws::si_wave_get_icon_model(m, a.v("w")).into()),
        ("SI_waveGetNextAttachedModel", |m, _o, a| ws::si_wave_get_next_attached_model(m, a.v("w"), a.v("prev")).into()),
        ("waveGetNextWave", |m, _o, a| ws::wave_get_next_wave(m, a.v("model"), a.v("wave")).into()),
        ("modelGetWaveName", |m, _o, a| ws::model_get_wave_name(m, a.v("node")).into()),
        ("grafnodeNextWave", |m, _o, a| ws::grafnode_next_wave(m, a.v("w")).into()),
        ("modelNextWave", |m, _o, a| ws::model_next_wave(m, a.v("w")).into()),
        ("grafnodeGetWaveName", |m, _o, a| ws::grafnode_get_wave_name(m, a.v("w")).into()),
        ("waveIsAttach", |m, _o, a| ws::wave_is_attach(m, a.v("w")).into()),
        ("waveIsDirty", |_m, _o, _a| ws::wave_is_dirty().into()),
    ])
    .route(route).effect(effect).given_lib_column()
    .run()
}
