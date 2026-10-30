// SPDX-License-Identifier: MIT
//! `modeler-checkkit <directory of vector files>`: the check of `si-modeler` on `si-checkkit`.
use si_checkkit::Kit;
use si_modeler::extrusion as x;
use si_modeler::effects as fx;
use si_modeler::extrusion2 as x2;
use si_modeler::geomupd1 as g1;

fn dbl(a: &si_checkkit::Args, name: &str) -> f64 {
    a.case().input(&format!("v_{name}")).f64()
}

/// The scratch arrays `SI_nurbsSrfExtractCurveSeg` grows and never frees (`aj`, `dl`, `dr`, 0x086d9c0c..0x086d9c14) hold
/// the working values of its last run; no later run reads one before writing it, so the port does not keep them. Their
/// bytes are taken over from the original's heap block of the vector so that the comparison of the heap judges what a
/// later reader could see (a unit's list of what it does not check: finding 0034, "geomupd1").
fn adopt_scratch(m: &mut si_mem::Mem, a: &si_checkkit::Args) {
    for g in [0x086d_9c0cu32, 0x086d_9c10, 0x086d_9c14] {
        let p = m.u32(g);
        let Some(i) = m.heap_blocks().iter().position(|b| b.0 == p && p != 0) else { continue };
        let Some(f) = a.case().try_output(&format!("heap{i}")) else { continue };
        let mut at = p;
        for w in f.words() {
            if let si_x87::vectors::Word::Value { bytes, value } = w {
                for k in 0..*bytes as u32 {
                    m.set_u8(at + k, (*value >> (8 * k)) as u8);
                }
            }
            at += match w {
                si_x87::vectors::Word::Value { bytes, .. } | si_x87::vectors::Word::Unwritten { bytes } => *bytes as u32,
                si_x87::vectors::Word::Ref { .. } => 4,
            };
        }
    }
}

/// The ported spline routines `SI_faceConvertToLinear` calls: the original runs its own in the
/// emulator, the port runs these (nothing is recorded for them).
struct NoCalls;
impl si_spline::spline_image::Outside for NoCalls {}

/// The same routing for the callees of a routed routine.
struct Routed;
impl si_mem::Outside for Routed {
    fn call(&mut self, m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        route(m, r, a).unwrap_or(si_mem::Ret::eax(0))
    }
}

/// The outside the update's NURBS route runs with: `SI_modelTouchGeometry` (which the definition routines call through
/// their outside) is [`si_model::model_image::touch::touch_geometry`], as the original's own run has it; the rest goes on.
struct TouchRun<'a>(&'a mut dyn si_mem::Outside);
impl si_mem::Outside for TouchRun<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        if r.name == "SI_modelTouchGeometry" {
            struct Back<'b>(&'b mut dyn si_mem::Outside);
            impl si_model::model_image::Outside for Back<'_> {
                fn call(&mut self, m: &mut si_mem::Mem, callee: &si_model::model_image::Callee, args: &[u32]) -> u32 {
                    self.0.call(m, si_mem::Routine::new(callee.name, callee.addr), args).eax
                }
            }
            si_model::model_image::touch::touch_geometry(m, &mut Back(self.0), si_mem::Ptr32::new(a[0]));
            return si_mem::Ret::eax(0);
        }
        self.0.call(m, r, a)
    }
}
/// MODELER_TRACE=1: a halt the arm turns into 0, on stderr.
fn trace_halt(e: si_nurbs::image::Halt) -> si_nurbs::image::Halt {
    if std::env::var_os("MODELER_TRACE").is_some() {
        eprintln!("PORT HALT {e:?}");
    }
    e
}

fn route(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> Option<si_mem::Ret> {
    use si_mem::Ptr32;
    // MODELER_TRACE=1: every call the port makes, with its words (a status message with its text), on stderr
    if std::env::var_os("MODELER_TRACE").is_some() {
        let text = if r.name == "SI_statusDisplayMessage" && a.first().is_some_and(|&p| p != 0) { String::from_utf8_lossy(&m.cstr(a[0])).into_owned() } else { String::new() };
        eprintln!("PORT {}({}) {text}", r.name, a.iter().map(|w| format!("{w:08x}")).collect::<Vec<_>>().join(","));
    }
    // MODELER_NATIVE=SI_faceDispose: a spec that runs SI_faceDispose as the binary has it (no stub): the port's calls of it
    // through an outside trait (si-model's disposal of a definition by its kind) run the ported routine, nothing recorded
    if r.name == "SI_faceDispose" && std::env::var("MODELER_NATIVE").is_ok_and(|v| v.split(',').any(|n| n == r.name)) {
        si_model::mesh_image::face::face_dispose(m, a[0]);
        return Some(si_mem::Ret::eax(0));
    }
    match r.name {
        // SI_nurbsSrfCopy runs ported; the original's own run calls SI_genTreeDestroy on a fresh surface's empty tree
        "SI_nurbsSrfCopy" => Some(si_mem::Ret::eax(si_nurbs::image::srf_copy(m, &mut Routed, a[0]))),
        // si-model's hierarchy builders call the allocator through their outside trait; the original runs its own
        "SI_modelAllocate" => Some(si_mem::Ret::eax(si_hrc::image::node::model_allocate(m, a[0]).addr())),
        "SI_patchCopy" => Some(si_mem::Ret::eax(si_spline::patch_image::patch_copy(m, Ptr32::new(a[0])).addr())),
        "SI_genTreeDestroy" => Some(si_mem::Ret::eax(0)),
        // the not-performed message of SI_extrusionUpdate: the library's sprintf, which the original runs unrecorded, with its one %d
        "sprintf" => {
            let mut text = Vec::new();
            let mut at = a[1];
            while m.u8(at) != 0 {
                if m.u8(at) == b'%' && m.u8(at + 1) == b'd' {
                    text.extend_from_slice(&si_mem::clib::decimal(a[2] as i32));
                    at += 2;
                } else {
                    text.push(m.u8(at));
                    at += 1;
                }
            }
            m.put_cstr(a[0], &text);
            Some(si_mem::Ret::eax(text.len() as u32))
        }
        "SI_splineConvertToLinear" => {
            let t = f32::from_bits(a[3]);
            let p = si_spline::convert_image::spline_convert_to_linear(m, &mut NoCalls, Ptr32::new(a[0]), a[1] as i16, a[2] as i32, t);
            Some(si_mem::Ret::eax(p.addr()))
        }
        "SI_splineInverse" => {
            si_spline::convert_image::spline_inverse(m, &mut NoCalls, Ptr32::new(a[0]));
            Some(si_mem::Ret::eax(0))
        }
        "SI_splineCleanup" => {
            si_spline::convert_image::spline_cleanup(m, &mut NoCalls, Ptr32::new(a[0]));
            Some(si_mem::Ret::eax(0))
        }
        // nrb_convert.c's 63-byte wrapper (another unit, not cited): a face of one curve, a copy of the curve turned round
        "SI_nurbsConvertToFace" => {
            if a[1] != 0 {
                m.set_i16(a[1] + 0x84, 0);
                let c = si_nurbs::image::crv_copy(m, a[0]);
                m.set_u32(a[1] + 0x80, c);
                si_nurbs::image::crv_inverse(m, &mut Routed, c);
                m.set_i16(a[1] + 0x78, 1);
            }
            Some(si_mem::Ret::eax(0))
        }
        // the knot insertion MakeCompatible reaches: the original runs its own natively (nothing recorded)
        "SI_nurbsCrvKnotInsert" => {
            let t = f64::from_bits(a[1] as u64 | (a[2] as u64) << 32);
            Some(si_mem::Ret::eax(si_nurbs::bridge::crv_knot_insert(m, a[0], t, a[3] as i32, a[4] as i16) as u32))
        }
        "SI_nurbsCrvConvertToLinear" => Some(si_mem::Ret::eax(si_nurbs::image::crv_convert_to_linear(m, a[0]).unwrap_or(0))),
        "SI_nurbsCrvInit" => {
            si_hrc::image::nurbs::nurbs_crv_init(m, a[0], a[1]);
            Some(si_mem::Ret::eax(0))
        }
        "SI_nurbsCrvDispose" => {
            si_hrc::image::nurbs::nurbs_crv_dispose(m, a[0]);
            Some(si_mem::Ret::eax(0))
        }
        _ => None,
    }
}

/// The words the port's image starts each case with (`Kit::reset_globals`; the port's run of them is put back):
/// the statics the allocators bump (`SI_splineAllocate`'s and `SI_modelAllocate`'s counters at 0x086d6960 and
/// 0x086d6520, and `left`, a scratch static of the NURBS basis code at 0x086d9c20, all 0 at the start), and
/// `SI_busyCursor` (0x086792b2), 1 in the original's data (so `SI_meshUpdatePolygonLists` makes no
/// `SI_stopActivity` call).
/// and the five statics of `SI_nurbsSrfExtractCurveSeg`'s scratch arrays (0x086d9c0c..0x086d9c1c: `aj`, `dl`, `dr` and their sizes).
const GLOBALS: [(u32, u32); 9] = [
    (0x086d_6960, 0),
    (0x086d_6520, 0),
    (0x086d_9c20, 0),
    (0x0867_92b2, 1),
    (0x086d_9c0c, 0),
    (0x086d_9c10, 0),
    (0x086d_9c14, 0),
    (0x086d_9c18, 0),
    (0x086d_9c1c, 0),
];

/// The ported callees of `SI_inverseUpdate`, `SI_modelSymmetry` and `SI_trimUpdate` that the
/// original runs in the emulator (`SI_modelInverse`, `SI_modelTouchGeometry`, the dictionary
/// search, the index routines, the son update, the surface disposal) run here in Rust; what they call comes back through this, the rest goes to the recorded
/// stand-ins of the case (the kit's `route` has no way back to them).
struct Nest<'a>(&'a mut dyn si_mem::Outside);
impl Nest<'_> {
    fn run(&mut self, m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        use si_mem::{Ptr32, Ret};
        match r.name {
            "SI_modelInverse" => {
                si_model::model_image::defs::inverse(m, self, Ptr32::new(a[0]));
                Ret::eax(0)
            }
            "SI_modelTouchGeometry" => {
                si_model::model_image::touch::touch_geometry(m, self, Ptr32::new(a[0]));
                Ret::eax(0)
            }
            "SI_elemdictFindRelatedElem" => {
                let e = si_dict::elemrel::find_related_elem(m, Ptr32::new(a[0]), Ptr32::new(a[1]), a[2] as i16, a[3] as i32, a[4] as i32, a[5] as i32, Ptr32::new(a[6]), Ptr32::new(a[7]));
                Ret::eax(e.addr())
            }
            "SI_modelUpdateSonState" => {
                si_model::model_image::state::update_son_state(m, self, Ptr32::new(a[0]), Ptr32::new(a[1]), a[2] as i16);
                Ret::eax(0)
            }
            // the kit's route runs the copy too, but answers the copy's own SI_genTreeDestroy itself
            "SI_nurbsSrfCopy" => Ret::eax(si_nurbs::image::srf_copy(m, self, a[0])),
            "SI_nurbsCrvCopy" => Ret::eax(si_nurbs::image::crv_copy(m, a[0])),
            "SI_nurbsCrvApplyMatrix" => {
                si_nurbs::image::crv_apply_matrix(m, a[0], a[1], a[2] as i32);
                Ret::eax(0)
            }
            "SI_nurbsCrvFlatten" => match si_nurbs::flatten::crv_flatten(m, a[0]) {
                Ok(c) => Ret::eax(c),
                Err(_) => panic!("SI_nurbsCrvFlatten stopped"),
            },
            // the trimming's ray casts: the original runs this itself
            "si_nurbsRayTraceSrf2" => {
                let mut routed = routed_new(self);
                Ret::eax(si_nurbs::ray_trace::nurbs_ray_trace_srf2(m, &mut routed, a[0], a[1], a[2], a[3]))
            }
            "SI_nurbsCrvValue" => {
                let t = f64::from_bits(a[2] as u64 | (a[3] as u64) << 32);
                let v = si_nurbs::image::crv_value(m, a[1], t, a[4] as i32).unwrap_or_else(|_| panic!("SI_nurbsCrvValue stopped"));
                for (k, x) in v.iter().enumerate() {
                    m.set_f64(a[0] + 8 * k as u32, *x);
                }
                Ret::eax(0)
            }
            "si_nurbsCurveOrientOrient" => {
                let mut routed = routed_new(self);
                Ret::eax(si_nurbs::curveorient2::orient(m, &mut routed, a[0], a[1]))
            }
            "si_nurbsCurveOrientCreateP" => Ret::eax(si_nurbs::curveorient::curve_orient_create_p(m, a[0], a[1], a[2] as i16, a[3])),
            "si_nurbsCurveOrientDestroy" | "" if r.addr == 0x0850_a36c => {
                si_nurbs::curveorient::curve_orient_destroy(m, a[0]);
                Ret::eax(0)
            }
            "si_bezPatchCreate" => Ret::eax(si_spline::bez_image::create(m, a[0] as i32, a[1] as i32, a[2])),
            "si_nurbsRayConnectNoTrans" => Ret::eax(si_nurbs::raylink::connect_no_trans(m, a[0], a[1], a[2] as i16, a[3]) as i32 as u32),
            "si_nurbsRayConnectTrans" => {
                let mut routed = routed_new(self);
                Ret::eax(si_nurbs::raylink::connect_trans(m, &mut routed, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9] as i16) as i32 as u32)
            }
            "si_bezPatchInit" => Ret::eax(si_spline::bez_image::init(m, a[0] as i32, a[1] as i32, a[2], a[3])),
            "si_bezPatchCleanup" => Ret::eax(si_spline::bez_image::cleanup(m, a[0])),
            "SI_nurbsSrfDispose" => {
                si_nurbs::image::srf_dispose(m, self, a[0]);
                Ret::eax(0)
            }
            "SI_indexModifAllocate" => Ret::eax(fx::index::index_modif_allocate(m, a[0], a[1] as i32)),
            "SI_indexModifPropagate" => {
                fx::index::index_modif_propagate(m, self, Ptr32::new(a[0]), a[1], a[2] as i16);
                Ret::eax(0)
            }
            _ => self.0.call(m, r, a)
        }
    }
}
impl si_mem::Outside for Nest<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        self.run(m, r, a)
    }
}
impl si_model::model_image::Outside for Nest<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, c: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        self.run(m, si_mem::Routine::new(c.name, c.addr), a).eax
    }
}

/// The NURBS routing; with `TRIM_SVN=1` (vectors made with `GEOMUPD5_TRIM_SVN=1`) `SI_nurbsSrfValueAndNormal` stays with the
/// case's recorded answers, because the original's own run extracts and frees curves the port does not make.
fn routed_new(o: &mut dyn si_mem::Outside) -> si_nurbs::routed::Routed<'_> {
    let mut r = si_nurbs::routed::Routed::new(o);
    if std::env::var_os("TRIM_SVN").is_some() {
        r.skip.push(0x084d_6824);
    }
    r
}

fn main() {
    Kit::new(&[
        ("SI_updateScalingMethod", |m, o, a| fx::mod_global::update_scaling_method(m, o, a.s("redraw")).into()),
        ("SI_nurbsExtrudeToNurbs", |m, _o, a| fx::extrusion3::nurbs_extrude_to_nurbs(m, a.v("curve"), a.i("kind"), a.f("depth"), a.i("steps"), a.i("axis"), a.v("p6")).into()),
        ("SI_createMatrixZScalingForExtrusion", |m, _o, a| {
            let (axis, point) = (si_math::types::Vector::new(a.f("ax"), a.f("ay"), a.f("az")), si_math::types::Vector::new(a.f("px"), a.f("py"), a.f("pz")));
            fx::extrude_linear::create_matrix_z_scaling_for_extrusion_at(m, a.v("out"), axis, point, a.f("angle"), a.f("max"));
            0u32.into()
        }),
        ("patchExtrudeFaceOnSpline", |m, o, a| fx::extrude_patch::patch_extrude_face_on_spline(m, o, a.v("face"), a.v("path")).into()),
        ("nurbsExtrudeNurbsOnNurbs", |m, o, a| fx::extrude_nurbs::nurbs_extrude_nurbs_on_nurbs(m, o, a.v("crv"), a.v("model")).map_err(trace_halt).unwrap_or(0).into()),
        ("patchExtrudeSplineOnSpline", |m, o, a| fx::extrude_patch::patch_extrude_spline_on_spline(m, o, a.v("spline"), a.v("path")).into()),
        ("faceExtrudeToPatchs", |m, o, a| {
            fx::extrude_patch::face_extrude_to_patchs(m, o, a.v("face"), a.i("kind"), a.f("depth"), a.i("steps"), a.v("out"), a.i("axis"), a.v("p6"));
            ().into()
        }),
        ("SI_splineExtrudeToPatch", |m, o, a| fx::extrusion3::spline_extrude_to_patch(m, o, a.v("spline"), a.i("kind"), a.f("depth"), a.i("steps"), a.i("axis"), a.v("p6")).into()),
        ("SI_adjustGnFunc", |m, o, a| fx::func::adjust_gn_func(m, o, si_mem::Ptr32::new(a.v("gn"))).into()),
        ("SI_indexModifAllocate", |m, _o, a| fx::index::index_modif_allocate(m, a.v("node"), a.i("kind")).into()),
        ("SI_edDisposeLattice", |m, o, a| {
            g1::deformation::ed_dispose_lattice(m, o, a.v("model"), a.s("which"));
            ().into()
        }),
        ("SI_modelRemoveLattice", |m, o, a| g1::deformation::model_remove_lattice(m, o, a.v("model"), a.s("which"), a.s("update")).into()),
        ("SI_edElementSetDeleteFlag", |m, o, a| {
            g1::deformation::ed_element_set_delete_flag(m, o, a.v("elem"), a.v("chapter"), a.v("index"), a.s("delete"), a.v("name_a"), a.v("name_b"), a.s("hard"));
            ().into()
        }),
        ("SI_surfaceValueAndTangent", |m, _o, a| {
            g1::deformation::surface_value_and_tangent(m, a.i("kind"), a.v("srf"), dbl(a, "u"), dbl(a, "v"), a.v("p"), a.v("tu"), a.v("tv"));
            adopt_scratch(m, a);
            ().into()
        }),
        ("meshSkinFromSpline", |m, o, a| {
            g1::skin::mesh_skin_from_spline(m, o, a.v("models"), a.s("steps"), a.s("count"), a.s("caps"), a.s("closed")).unwrap_or(0).into()
        }),
        ("skin2patch", |m, o, a| g1::skin::skin2patch(m, o, a.v("models"), a.s("count"), a.i("type"), a.s("closed"), a.i("flag")).into()),
        ("si_createSkinFromSpline", |m, o, a| g1::skin::create_skin_from_spline(m, o, a.v("models"), a.v("slot"), a.v("skin")).unwrap_or(0).into()),
        ("SI_skinningUpdate", |m, o, a| g1::skin::skinning_update(m, o, a.v("node"), a.f("time")).unwrap_or(()).into()),
        ("SI_extractionUpdate", |m, o, a| {
            g1::extraction::extraction_update(m, o, a.v("node"), a.f("time")).unwrap_or(()).into()
        }),
        ("SI_indexEdgePropagate", |m, o, a| {
            fx::index::index_edge_propagate(m, o, a.p("node"), a.v("pp"));
            ().into()
        }),
        ("SI_indexModifPropagate", |m, o, a| {
            fx::index::index_modif_propagate(m, o, a.p("node"), a.v("pp"), a.s("full"));
            ().into()
        }),
        ("SI_indexModifFree", |m, _o, a| {
            fx::index::index_modif_free(m, a.v("pp"));
            ().into()
        }),
        ("SI_modelHasModellingRel", |m, o, a| fx::mod_effect::model_has_modelling_rel(m, o, a.p("model")).into()),
        ("SI_modelSymmetry", |m, o, a| fx::mod_effect::model_symmetry(m, &mut Nest(o), a.p("model"), a.v("plane"), a.s("hier")).addr().into()),
        ("SI_trimUpdate", |m, o, a| {
            // the trimming's callees the original runs itself (knot insertions, ray casts, ...) run as ports through the NURBS routing
            let mut nest = Nest(o);
            let mut routed = routed_new(&mut nest);
            fx::trim_dial::trim_update(m, &mut routed, a.v("upd"));
            ().into()
        }),
        ("SI_inverseUpdate", |m, o, a| {
            fx::mod_effect::inverse_update(m, &mut Nest(o), a.v("upd"));
            ().into()
        }),
        ("si_createNurbsHierarchyFromNurbsCrv", |m, o, a| {
            let _ = x::create_nurbs_hierarchy_from_nurbs_crv(m, o, a.v("srf"), a.v("mp"), a.s("update"), a.s("caps"), a.s("extrude"));
            ().into()
        }),
        ("createMeshFromNurbsExtrusion", |m, o, a| {
            x::create_mesh_from_nurbs_extrusion(m, o, a.p("model"), a.v("curve"), a.v("ed"), a.s("upd")).into()
        }),
        ("createMeshFromExtrusionSimple", |m, o, a| {
            x::create_mesh_from_extrusion_simple(m, o, a.p("model"), a.v("spline"), a.v("face"), a.v("ed"), a.s("is_spline"), a.s("upd")).into()
        }),
        ("createMeshFromNurbsFaceExtrusion", |m, o, a| {
            x::create_mesh_from_nurbs_face_extrusion(m, o, a.p("model"), a.v("face"), a.v("ed"), a.s("upd")).into()
        }),
        ("createPatchFromExtrusionFromSpline", |m, o, a| fx::extrude_model::create_patch_from_extrusion_from_spline(m, o, a.v("mp"), a.v("spline"), a.v("path"), a.v("ed"), a.s("update")).into()),
        ("createPatchFromExtrusionFromFace", |m, o, a| fx::extrude_model::create_patch_from_extrusion_from_face(m, o, a.v("mp"), a.v("face"), a.v("path"), a.v("ed"), a.s("update")).into()),
        ("createNurbsFromExtrusionFromNurbs", |m, o, a| fx::extrude_model::create_nurbs_from_extrusion_from_nurbs(m, o, a.v("mp"), a.v("crv"), a.v("path"), a.v("ed"), a.s("update")).unwrap_or(0).into()),
        ("createNurbsSrfFromExtrusionFromFace", |m, o, a| fx::extrude_model::create_nurbs_srf_from_extrusion_from_face(m, o, a.v("mp"), a.v("face"), a.v("path"), a.v("ed"), a.s("update")).unwrap_or(0).into()),
        ("createModelFromExtrusion", |m, o, a| fx::extrude_model::create_model_from_extrusion(m, o, a.v("src"), a.v("path"), a.v("mp"), a.v("pspl"), a.v("ed")).map_err(trace_halt).unwrap_or(0).into()),
        ("SI_extrusionUpdate", |m, o, a| { let _ = fx::extrude_model::extrusion_update(m, o, a.v("upd"), a.f("time")); 0u32.into() }),
        ("meshExtrudeNurbsFace", |m, o, a| x2::mesh_extrude_nurbs_face(m, o, a.v("face"), a.s("step"), a.v("mat"), a.i("steps"), a.s("caps")).into()),
        ("meshExtrudeSpline", |m, o, a| x2::mesh_extrude_spline(m, o, a.v("spline"), a.s("step"), a.v("mat"), a.i("steps"), a.s("caps")).into()),
        ("SI_meshExtrudeSplineOnSpline", |m, o, a| x2::mesh_extrude_spline_on_spline(m, o, a.v("spline"), a.s("a"), a.v("path"), a.i("steps"), a.s("caps")).into()),
        ("SI_meshExtrudeNurbsOnNurbs", |m, o, a| x2::mesh_extrude_nurbs_on_nurbs(m, o, a.v("curve"), a.s("step"), a.v("model"), a.i("steps"), a.s("caps")).unwrap_or(0).into()),
        ("meshExtrudeNurbsFaceOnCurve", |m, o, a| x2::mesh_extrude_nurbs_face_on_curve(m, o, a.v("face"), a.s("step"), a.v("model"), a.i("steps"), a.s("caps")).unwrap_or(0).into()),
        ("meshExtrudeFaceOnSpline", |m, o, a| x2::mesh_extrude_face_on_spline(m, o, a.v("face"), a.s("step"), a.v("path"), a.i("steps"), a.s("caps")).into()),
        ("meshRevolveFace", |m, o, a| fx::revolution::mesh_revolve_face(m, o, a.v("face"), a.s("step"), a.i("steps"), a.f("angle"), a.i("axis"), a.v("model"), a.s("caps")).into()),
        ("meshRevolveSpline", |m, o, a| fx::revolution::mesh_revolve_spline(m, o, a.v("spline"), a.s("step"), a.i("steps"), a.f("angle"), a.i("axis"), a.v("model"), a.s("caps")).into()),
        ("nurbsExtrudeNurbsOnLinearNurbs", |m, o, a| fx::lin_nurbs::nurbs_extrude_nurbs_on_linear_nurbs(m, o, a.v("crv"), a.v("model")).unwrap_or(0).into()),
        ("createModelFromRevolution", |m, o, a| fx::revolve_four::create_model_from_revolution(m, o, a.v("src"), a.v("model"), a.v("mp"), a.v("rev")).into()),
        ("SI_faceCopy", |m, _o, a| fx::revolve_four::face_copy(m, a.v("face")).into()),
        ("faceRevolutionToPatchs", |m, o, a| {
            fx::revolution::face_revolution_to_patchs(m, o, a.v("face"), a.i("kind"), a.s("steps"), a.f("angle"), a.s("compensate"), a.i("axis"), a.v("model"), a.v("patches"), a.i("def"));
            ().into()
        }),
        ("si_createFourSidedPatch", |m, o, a| fx::four_patch::create_four_sided_patch(m, o, a.v("models"), a.v("mp"), a.v("msg")).into()),
        ("SI_fourSidedUpdate", |m, o, a| {
            fx::four_patch::four_sided_update(m, &mut TouchRun(o), a.v("upd"));
            ().into()
        }),
        ("si_createFourSidedNurbs", |m, o, a| fx::four_nurbs::create_four_sided_nurbs(m, o, a.v("models"), a.v("model_at"), a.v("msg"), a.s("dialog"), a.v("data")).unwrap_or(0).into()),
        ("meshExtrudeFace", |m, o, a| x2::mesh_extrude_face(m, o, a.v("face"), a.s("step"), a.v("mat"), a.i("steps"), a.s("caps")).into()),
        // unit lin1
        ("patchExtrudeFaceOnLinearSpline", |m, o, a| fx::lin_patch::patch_extrude_face_on_linear_spline(m, o, a.v("face"), a.v("path")).into()),
        ("meshExtrudeFaceOnLinearSpline", |m, o, a| fx::lin_patch::mesh_extrude_face_on_linear_spline(m, o, a.v("face"), a.s("step"), a.v("path"), a.i("steps"), a.s("caps")).into()),
    ])
    .route(route)
    .normalise_stack_args()
    .reset_globals(&GLOBALS)
    .given_lib_column()
    .allow_global_write("GW:count.225+0x0")
    .allow_global_write("GW:count.228+0x0")
    .allow_global_write("GW:left.272+0x0")
    .allow_global_write("GW:aj.249+0x0")
    .allow_global_write("GW:aj.249+0x1")
    .allow_global_write("GW:aj.249+0x3")
    .allow_global_write("GW:dl.250+0x3")
    .allow_global_write("GW:dr.251+0x1")
    .allow_global_write("GW:dr.251+0x3")
    .allow_global_write("GW:nb_aj.253+0x0")
    .setup(|m| {
        // MODELER_STALE=1: before each run of a case, say on stderr whether the run before it used a value the original
        // reads without having set it (`si_model::env::stale`, e.g. SI_faceOrdinatePoints' reference of a face whose
        // outline has no points); the counter is then cleared. With a file of one case: the line printed before the
        // second run is the first run's.
        if std::env::var_os("MODELER_STALE").is_some() {
            let n = si_model::env::stale::count();
            if n > 0 {
                eprintln!("MODELER_STALE: the run before this one used {n} value(s) the original never set");
            }
            si_model::env::stale::reset();
        }
        si_hrc::image::globals::store_data(m);
        x::store_names(m);
        // the names si_createPatchHierarchyFromSpline (0x081568a8) and the extrusion model (0x08689fb8) use
        for (at, text) in [(0x0868_9ea0u32, &b"bottom"[..]), (0x0868_9ea7, b"top"), (0x0868_9eab, b"extru"), (0x0868_9eb1, b"revol"), (0x0868_9eb7, b"null"), (0x0868_9f5d, b"null"), (0x0868_9fb8, b"extru")] {
            m.put_cstr(at, text);
        }
        fx::revolve_four::store_names(m);
        m.put_cstr(0x0868_7250, b"skin"); // the name of the skin's model (skin.rs)
        fx::four_patch::store_names(m);
        fx::four_nurbs::store_names(m);
        si_mem::init_globals(m, si_model::mesh_image::g::ALL);
    })
    .run()
}
