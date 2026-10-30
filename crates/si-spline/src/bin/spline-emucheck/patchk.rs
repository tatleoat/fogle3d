// SPDX-License-Identifier: MIT
//! The checks of the patch routines (`patch.c`).

use si_math::types::Matrix;
use si_spline::patch::{self, Surface};
use si_spline::patch_edit;
use si_spline::patch_grid::{self, MeshHost, PatchMesh};
use si_x87::vectors::{Case, Check, Verdict, Word};

use crate::pat::*;
use crate::util::*;

fn mat(c: &Case, name: &str) -> Matrix {
    let f = c.input(name).f32s();
    core::array::from_fn(|i| core::array::from_fn(|j| f[4 * i + j]))
}

fn flat(m: &Matrix) -> Vec<u32> {
    m.iter().flatten().map(|f| f.to_bits()).collect()
}

/// The mesh host of a case: the texture query answers what the recorded call answered.
struct Mesh<'a> {
    case: &'a Case,
    log: Vec<String>,
}

impl MeshHost for Mesh<'_> {
    fn has_texture(&mut self) -> bool {
        let r = self.case.calls.iter().find(|k| k.name == "SI_mdlGetMatAndTxt2D").map(|k| k.result.value().unwrap_or(0)).unwrap_or(0);
        self.log.push("SI_mdlGetMatAndTxt2D".into());
        if r != 0 {
            self.log.push("SI_genListDestroy".into());
        }
        r != 0
    }
    fn nb_pol_max(&self) -> i32 {
        self.case.input("polmax").i32()
    }
    fn finish(&mut self, _mesh: &mut PatchMesh) {
        self.log.extend(["SI_meshUpdate".to_string(), "SI_meshInitVertexColors".to_string(), "SI_newMeshCleanup".to_string()]);
    }
}

/// A capping the display update would run leaves the block of points: the port refuses
/// it, the original reads and writes other memory.
fn caps_leave_block(s: &Surface) -> bool {
    let r = &s.rec;
    if r.unk_1e4 & 1 != 0 {
        return false;
    }
    [(r.v_bottom, r.v_closed, 0), (r.v_top, r.v_closed, 1), (r.u_bottom, r.u_closed, 2), (r.u_top, r.u_closed, 3)]
        .iter()
        .any(|(flag, closed, which)| *flag != 0 && *closed == 0 && !patch_grid::capping_in_block(s, *which))
}

/// The index table of a case (`t`), when it was handed in.
fn table(c: &Case) -> Option<Vec<i32>> {
    let f = c.input("t");
    if f.is_null() {
        None
    } else {
        Some(nums(f).iter().map(|w| w.unwrap_or(0) as i32).collect())
    }
}

fn same_table(c: &Case, t: &Option<Vec<i32>>) -> Verdict {
    match (t, c.try_output("t")) {
        (Some(t), Some(f)) => same("t", f, &t.iter().map(|v| *v as u32).collect::<Vec<_>>()),
        // no table: nothing to compare (the mutation run must still see a difference)
        (None, _) => Verdict::from(!mutate(), || "mutated".into()),
        (Some(_), None) => Verdict::Different("the table is not in the outputs".into()),
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_matrixDerivativePrecision" => |c| same_out(c, "m", &flat(&patch_grid::matrix_derivative_precision(c.input("n").i16()))),
        "SI_patchUpdateMatrix" => |c| {
            let mut s = surface(c, "");
            if !(0..=3).contains(&s.rec.u_type) {
                // the U basis is whatever the stack held: nothing of the result is defined
                // but the two step counts (and two garbage values can agree by chance)
                return Verdict::Skip;
            }
            patch::update_matrix(&mut s);
            same("pat", c.output("pat"), &pat_words(&s.rec))
        },
        "SI_patchNbTriangles" => |c| same_eax(c, patch::nb_triangles(&surface(c, "")) as u32),
        "SI_patchValue" => |c| match patch::value(&surface(c, ""), c.input("u").f32(), c.input("v").f32()) {
            Some(p) => same_out(c, "out", &vw(p)),
            None => Verdict::Different("the port refuses a case the original answered".into()),
        },
        "SI_patchValueAndTangent" => |c| match patch::value_and_tangent(&surface(c, ""), c.input("u").f32(), c.input("v").f32()) {
            Some(t) => same_eax(c, 1).and(|| same_out(c, "p", &vw(t.point))).and(|| same_out(c, "tu", &vw(t.tu))).and(|| same_out(c, "tv", &vw(t.tv))),
            None => same_eax(c, 0).and(|| unwritten(c, "p")).and(|| unwritten(c, "tu")).and(|| unwritten(c, "tv")),
        },
        "SI_patchValueAndNormal" => |c| match patch::value_and_normal(&surface(c, ""), c.input("u").f32(), c.input("v").f32()) {
            Some((p, n)) => same_eax(c, 1).and(|| same_out(c, "p", &vw(p))).and(|| same_out(c, "n", &vw(n))),
            // the tangent routine refused: the point is left alone and the normal is made
            // of the caller's uninitialised locals
            None => same_eax(c, 1).and(|| unwritten(c, "p")),
        },
        "SI_patchTransform" => |c| {
            let mut s = surface(c, "");
            patch::transform(&mut s, &mat(c, "m"));
            same_surface(c, "", "o", &s)
        },
        "SI_patchTransferPoints" => |c| {
            let mut to = surface(c, "2");
            patch::transfer_points(&surface(c, ""), &mut to);
            same_surface(c, "2", "o2", &to)
        },
        "SI_patchForceCapping" => |c| {
            let mut s = surface(c, "");
            if !patch_grid::force_capping(&mut s, c.input("which").i16(), c.input("avg").i16()) {
                return Verdict::Skip; // the original works outside the block of points
            }
            same_surface(c, "", "o", &s)
        },
        "SI_patchSurface" => |c| {
            let Some(g) = patch_grid::patch_surface(&surface(c, "")) else { return Verdict::Skip };
            let null = matches!(c.output("out").words().first(), Some(Word::Value { value: 0, .. }));
            let w: Vec<u32> = g.points.iter().flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect();
            Verdict::from(null == g.points.is_empty(), || format!("block: original null {null}, port {} points", g.points.len()))
                .and(|| same_out(c, "nu", &[g.nu as u16 as u32]))
                .and(|| same_out(c, "nv", &[g.nv as u16 as u32]))
                .and(|| same_out(c, "grid", &w))
        },
        "SI_patchUpdateSurface" => |c| {
            let mut s = surface(c, "");
            if caps_leave_block(&s) {
                return Verdict::Skip;
            }
            patch_grid::update_surface(&mut s);
            same_surface(c, "", "o", &s)
        },
        "SI_patchComputeSurface" => |c| {
            let mut s = surface(c, "");
            if caps_leave_block(&s) {
                return Verdict::Skip;
            }
            patch_grid::compute_surface(&mut s, c.input("mode").i16());
            same_surface(c, "", "o", &s)
        },
        "SI_patchComputesSubdivision" => |c| {
            let mut s = surface(c, "");
            if !patch_grid::computes_subdivision(&mut s) {
                return Verdict::Skip;
            }
            same_surface(c, "", "o", &s)
        },
        "SI_patchCopy" => |c| {
            let n = patch::copy(&surface(c, ""));
            same_new_pat(c, "n.rec", &n.rec).and(|| same_arrays(c, "n", &n))
        },
        "SI_patchDispose" => |c| {
            let Some(rec) = c.try_input("pat") else {
                return Verdict::from(c.try_output("freed").is_none() && !mutate(), || "a null pointer freed something".into());
            };
            let w = nums(rec);
            let mut want = Vec::new();
            for (i, name) in [(109, "pts"), (110, "tags"), (113, "s0"), (129, "m0"), (116, "s1"), (130, "m1"), (121, "d0"), (122, "d1")] {
                if w[i] != Some(0) {
                    want.push(name.to_string());
                }
            }
            want.push("pat".to_string());
            let got: Vec<String> = c.output("freed").words().iter().map(|w| match w {
                Word::Ref { name, .. } => name.clone(),
                _ => "?".to_string(),
            }).collect();
            let mut s: Option<Surface> = Some(surface(c, ""));
            patch::dispose(&mut s);
            Verdict::from(want == got && s.is_none() && !mutate(), || format!("freed: original {got:?}, expected {want:?}")).and(|| same_out(c, "pp", &[0]))
        },
        "SI_patchInverse" => |c| {
            let mut s = surface(c, "");
            if caps_leave_block(&s) {
                return Verdict::Skip;
            }
            let mut t = table(c);
            if !patch_edit::inverse(&mut s, t.as_deref_mut()) {
                return Verdict::Skip; // the original exchanges memory outside its arrays
            }
            same_surface(c, "", "o", &s).and(|| same_table(c, &t))
        },
        "SI_patchPerformControls" => |c| {
            let mut s = surface(c, "");
            if caps_leave_block(&s) {
                return Verdict::Skip;
            }
            let op = c.input("op").i16();
            if op != 0 && op != 1 {
                // the exchange of U and V rebuilds the display with the flags exchanged
                let mut t = s.clone();
                (t.rec.u_bottom, t.rec.v_bottom, t.rec.u_top, t.rec.v_top) = (s.rec.v_bottom, s.rec.u_bottom, s.rec.v_top, s.rec.u_top);
                (t.rec.nb_u, t.rec.nb_v, t.rec.u_type, t.rec.v_type) = (s.rec.nb_v, s.rec.nb_u, s.rec.v_type, s.rec.u_type);
                (t.rec.u_closed, t.rec.v_closed) = (s.rec.v_closed, s.rec.u_closed);
                if caps_leave_block(&t) {
                    return Verdict::Skip;
                }
            }
            let mut t = table(c);
            if !patch_edit::perform_controls(&mut s, op, t.as_deref_mut()) {
                return Verdict::Skip;
            }
            same_surface(c, "", "o", &s).and(|| same_table(c, &t))
        },
        "pInvKltUpdate" => |c| {
            let s = surface(c, "");
            let mut t = table(c);
            patch_edit::inv_klt_update_for_check(&s, t.as_deref_mut());
            same_table(c, &t)
        },
        "pchInvReorderCV" => |c| {
            let mut s = surface(c, "");
            patch_edit::inv_reorder_cv_for_check(&mut s);
            same_surface(c, "", "o", &s)
        },
        "SI_patchDeleteRow" => |c| {
            let mut s = surface(c, "");
            if caps_leave_block(&s) {
                return Verdict::Skip;
            }
            let mut t = table(c);
            match patch_edit::delete_row(&mut s, c.input("index").i16(), c.input("along").i16(), t.as_deref_mut()) {
                None => Verdict::Skip, // the original overruns its new block
                Some(r) => same_eax(c, r as u32).and(|| same_surface(c, "", "o", &s)).and(|| same_table(c, &t)),
            }
        },
        "SI_patchExtractSpline" => |c| {
            let mut n = 0;
            match patch_edit::extract_spline(&surface(c, ""), c.input("which").i16(), c.input("t").f32(), &mut n) {
                Some(got) => same_new(c, &Some(got)),
                None => Verdict::Skip, // rows outside the block, or a shrink the original faults in
            }
        },
        "SI_patchRandomize" => |c| {
            let mut s = surface(c, "");
            if caps_leave_block(&s) {
                return Verdict::Skip;
            }
            let a = c.input("amp").f32s();
            // the harness's generator starts every case from this state
            let mut g = patch_edit::Drand48(0x1234_ABCD_330E);
            patch_edit::randomize(&mut s, &si_spline::Vector { x: a[0], y: a[1], z: a[2] }, c.input("count").i16(), c.input("tagged").i16(), &mut g);
            same_surface(c, "", "o", &s)
        },
        "SI_patchConvertToMesh" => |c| {
            // SI_meshAllocate computes the cosine of its default angle: the tape has it
            let _ = si_x87::trig::cos(f64::from_bits(0x3ff0_c152_37db_38a0));
            let mut h = Mesh { case: c, log: Vec::new() };
            let got = patch_grid::convert_to_mesh(&surface(c, ""), &mut h);
            let null = matches!(c.output("ptr").words().first(), Some(Word::Value { value: 0, .. }));
            let calls: Vec<String> = c.calls.iter().filter(|k| k.name.starts_with("SI_")).map(|k| k.name.clone()).collect();
            let Some(m) = got else {
                // the port also refuses what the original cannot finish; when it returned
                // null it disposed of the mesh after the texture query
                if !null {
                    return Verdict::Skip;
                }
                h.log.push("SI_meshDispose".into());
                return Verdict::from(calls == h.log && !mutate(), || format!("calls: original {calls:?}, port {:?}", h.log));
            };
            if null {
                return Verdict::Different("the original returned null, the port a mesh".into());
            }
            let verts: Vec<u32> = m.vertices.iter().flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect();
            let pols: Vec<u32> = m.polygons.iter().flat_map(|p| [4u32, p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32]).collect();
            let uvs: Vec<u32> = match &m.uvs {
                Some(u) => u.iter().flatten().flatten().map(|f| f.to_bits()).collect(),
                // SI_polygonNodeAllocate clears the nodes
                None => vec![0; m.polygons.len() * 8],
            };
            Verdict::from(calls == h.log, || format!("calls: original {calls:?}, port {:?}", h.log))
                .and(|| same_out(c, "mesh.counts", &[m.vertices.len() as u32, m.polygons.len() as u32]))
                .and(|| same_out(c, "mesh.verts", &verts))
                .and(|| same_out(c, "mesh.pols", &pols))
                .and(|| same_out(c, "mesh.uvs", &uvs))
        },
        _ => return None,
    })
}
