// SPDX-License-Identifier: MIT
//! The patch part of the corpus check: every patch of the sample models through the patch
//! code. No original to compare with here; the checks are the port against itself and
//! "no panic".

use std::collections::BTreeMap;

use si_spline::patch::{self, Surface};
use si_spline::patch_edit;
use si_spline::patch_grid::{self, NoMeshHost};
use si_spline::Vector;

#[derive(Default)]
pub struct PTally {
    pub patches: u32,
    pub points: u64,
    pub cells: u64,
    pub no_cells: u32,
    pub non_finite: u32,
    pub capped: u32,
    pub short_of_points: u32,
    pub by_step: BTreeMap<(u16, u16), u32>,
    pub corner_is_point: (u64, u64),
    pub wrap_same: (u64, u64),
    pub tangent_point_is_value: (u64, u64),
    pub tangent_matches_difference: (u64, u64),
    pub normal_is_unit: (u64, u64),
    pub grid_counts: (u32, u32),
    pub grid_on_surface: (u32, u32),
    pub mesh_counts: (u32, u32),
    pub shading_counts: (u32, u32),
    pub shading_equals_grid: (u64, u64),
    pub shading_normals_unit: (u64, u64),
    pub display_counts: (u32, u32),
    pub display_on_surface: (u32, u32),
    pub partial_update_same: (u32, u32),
    pub inverse_twice: (u32, u32),
    pub inverse_display_same: (u32, u32),
    pub extract_on_surface: (u32, u32),
    pub copy_same: (u32, u32),
    pub quads: u64,
}

fn ok(t: &mut (u32, u32), good: bool) {
    t.1 += 1;
    t.0 += good as u32;
}

fn ok64(t: &mut (u64, u64), good: bool) {
    t.1 += 1;
    t.0 += good as u64;
}

fn dist(a: Vector, b: Vector) -> f64 {
    let d = [(a.x - b.x) as f64, (a.y - b.y) as f64, (a.z - b.z) as f64];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn bits(v: Vector) -> [u32; 3] {
    [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

pub fn label(p: &si_hrc::Patch) -> String {
    let name = |k: i32| match k {
        0 => "linear",
        1 => "Bezier",
        2 => "B-spline",
        3 => "cardinal",
        _ => "other",
    };
    let oc = |c: i16| if c != 0 { "closed" } else { "open" };
    format!("{} {} x {} {}", name(p.kind[0]), oc(p.close[0]), name(p.kind[1]), oc(p.close[1]))
}

pub fn check(p: &si_hrc::Patch, t: &mut PTally, counter: &mut i32) {
    let mut s = Surface::from_file(p);
    t.patches += 1;
    t.points += s.points.len() as u64;
    t.short_of_points += (p.points.len() < p.nu as usize * p.nv as usize) as u32;
    *t.by_step.entry((s.rec.u_step, s.rec.v_step)).or_default() += 1;
    let capped = p.caps.iter().any(|c| *c != 0);
    t.capped += capped as u32;
    let ((nsu, stu), (nsv, stv)) = s.cells();
    let finite = s.points.iter().all(|q| q.iter().all(|f| f.is_finite()));
    // everything must at least run
    patch_grid::update_surface(&mut s);
    if nsu <= 0 || nsv <= 0 || !finite {
        t.no_cells += (nsu <= 0 || nsv <= 0) as u32;
        t.non_finite += (!finite) as u32;
        let _ = patch::value(&s, 0.5, 0.5);
        let _ = patch::value_and_normal(&s, 0.5, 0.5);
        let _ = patch_grid::patch_surface(&s);
        let _ = patch_grid::computes_subdivision(&mut s);
        let _ = patch_grid::convert_to_mesh(&s, &mut NoMeshHost);
        return;
    }
    t.cells += nsu as u64 * nsv as u64;
    let size = s.points.iter().flat_map(|q| q[..3].iter()).fold(1e-6f64, |m, f| m.max(f.abs() as f64));
    let tol = size * 1e-3 + 1e-5;
    let r = s.rec;
    let (us, vs) = (r.u_step as i32, r.v_step as i32);
    let interpolates = |k: i32| k == 0 || k == 1;

    // ---- evaluation
    for cu in 0..=nsu {
        for cv in 0..=nsv {
            if interpolates(r.u_type) && interpolates(r.v_type) {
                // a corner of a cell is a control point, to the last bit
                let i = (cu * stu) % r.nb_u as i32;
                let j = (cv * stv) % r.nb_v as i32;
                let q = s.points[(i * r.nb_v as i32 + j) as usize];
                let v = patch::value(&s, cu as f32, cv as f32);
                ok64(&mut t.corner_is_point, v.map(bits) == Some([q[0].to_bits(), q[1].to_bits(), q[2].to_bits()]));
            }
            if cu == nsu || cv == nsv {
                continue;
            }
            let (u, v) = (cu as f32 + 0.5, cv as f32 + 0.25);
            let Some(tan) = patch::value_and_tangent(&s, u, v) else { continue };
            if let Some(q) = patch::value(&s, u, v) {
                ok64(&mut t.tangent_point_is_value, dist(q, tan.point) <= tol);
            }
            if r.u_closed != 0 {
                let w = patch::value_and_tangent(&s, u + nsu as f32, v);
                let b = patch::value_and_tangent(&s, u - nsu as f32, v);
                ok64(&mut t.wrap_same, w == Some(tan) && b == Some(tan));
            }
            if r.v_closed != 0 {
                let w = patch::value_and_tangent(&s, u, v + nsv as f32);
                let b = patch::value_and_tangent(&s, u, v - nsv as f32);
                ok64(&mut t.wrap_same, w == Some(tan) && b == Some(tan));
            }
            let h = 1.0f32 / 64.0;
            if let (Some(a), Some(b)) = (patch::value(&s, u - h, v), patch::value(&s, u + h, v)) {
                let q = Vector { x: (b.x - a.x) / (2.0 * h), y: (b.y - a.y) / (2.0 * h), z: (b.z - a.z) / (2.0 * h) };
                ok64(&mut t.tangent_matches_difference, dist(tan.tu, q) <= size * 0.02 + 1e-5);
            }
            if let (Some(a), Some(b)) = (patch::value(&s, u, v - h), patch::value(&s, u, v + h)) {
                let q = Vector { x: (b.x - a.x) / (2.0 * h), y: (b.y - a.y) / (2.0 * h), z: (b.z - a.z) / (2.0 * h) };
                ok64(&mut t.tangent_matches_difference, dist(tan.tv, q) <= size * 0.02 + 1e-5);
            }
            if let Some((_, n)) = patch::value_and_normal(&s, u, v) {
                let l = (n.x as f64).hypot(n.y as f64).hypot(n.z as f64);
                // a unit vector, or the raw cross product where that is shorter than 0.0001
                ok64(&mut t.normal_is_unit, (l - 1.0).abs() < 1e-4 || l <= 1.1e-4);
            }
        }
    }

    // ---- the grid of points and the mesh
    let want_nu = us * nsu + (r.u_closed == 0) as i32;
    let want_nv = vs * nsv + (r.v_closed == 0) as i32;
    let grid = patch_grid::patch_surface(&s);
    ok(&mut t.grid_counts, grid.as_ref().is_some_and(|g| (g.nu as i32, g.nv as i32, g.points.len() as i32) == (want_nu, want_nv, want_nu * want_nv)));
    if let Some(g) = &grid {
        let mut on = true;
        for i in 0..g.nu as i32 {
            for j in 0..g.nv as i32 {
                let (u, v) = (i as f32 / us as f32, j as f32 / vs as f32);
                if let Some(q) = patch::value(&s, u, v) {
                    on &= dist(q, g.points[(i * g.nv as i32 + j) as usize]) <= tol;
                }
            }
        }
        ok(&mut t.grid_on_surface, on);
    }
    let mesh = patch_grid::convert_to_mesh(&s, &mut NoMeshHost);
    let quads = us * nsu * vs * nsv;
    match &mesh {
        Some(m) => {
            t.quads += m.polygons.len() as u64;
            let inside = m.polygons.iter().all(|q| q.iter().all(|i| (0..m.vertices.len() as i32).contains(i)));
            ok(&mut t.mesh_counts, m.polygons.len() as i32 == quads && inside && patch::nb_triangles(&s) == 2 * quads);
        }
        None => ok(&mut t.mesh_counts, quads <= 0),
    }

    // ---- the shading grid
    let mut shaded = s.clone();
    let done = patch_grid::computes_subdivision(&mut shaded);
    let per = ((us + 1) * (vs + 1)) as usize;
    ok(&mut t.shading_counts, done && shaded.subdiv0.len() == nsu as usize * nsv as usize * per && shaded.subdiv1.len() == shaded.subdiv0.len());
    if let Some(g) = &grid {
        for cu in 0..nsu {
            for cv in 0..nsv {
                for a in 0..us {
                    for b in 0..vs {
                        // the same differences from the same corner: the same bits
                        let k = ((cu * nsv + cv) as usize * (us + 1) as usize + a as usize) * (vs + 1) as usize + b as usize;
                        let q = g.points[((us * cu + a) * g.nv as i32 + vs * cv + b) as usize];
                        ok64(&mut t.shading_equals_grid, shaded.subdiv0.get(k).map(|v| bits(*v)) == Some(bits(q)));
                    }
                }
            }
        }
    }
    for n in &shaded.subdiv1 {
        let l = (n.x as f64).hypot(n.y as f64).hypot(n.z as f64);
        ok64(&mut t.shading_normals_unit, (l - 1.0).abs() < 1e-4 || l <= 1.1e-4);
    }

    // ---- the display curves
    let n0 = r.u_step.wrapping_mul(nsu as u16).wrapping_add(1) as usize * (r.u_nb_curves as usize * nsv as usize + (r.v_closed == 0) as usize);
    let n1 = r.v_step.wrapping_mul(nsv as u16).wrapping_add(1) as usize * (r.v_nb_curves as usize * nsu as usize + (r.u_closed == 0) as usize);
    ok(&mut t.display_counts, s.surface0.len() == n0 && s.surface1.len() == n1);
    // curve k of the U-running set is at v = k / u_nb_curves; its point i at u = i / u_step
    let w = s.rec.surface0_dims[0] as usize;
    let mut on = true;
    if w > 0 && !capped {
        for (k, q) in s.surface0.iter().enumerate() {
            let (row, i) = (k / w, k % w);
            if let Some(v) = patch::value(&s, i as f32 / us as f32, row as f32 / r.u_nb_curves as f32) {
                on &= dist(v, *q) <= tol;
            }
        }
        ok(&mut t.display_on_surface, on);
    }
    // a partial update for tagged points, with every point tagged, writes what a full one does
    let mut tagged = s.clone();
    tagged.tags.iter_mut().for_each(|g| *g |= 1);
    patch_grid::compute_surface(&mut tagged, patch_grid::TAGGED);
    ok(&mut t.partial_update_same, tagged.surface0 == s.surface0 && tagged.surface1 == s.surface1);

    // ---- editing
    let mut inv = s.clone();
    let in_block = patch_edit::inverse(&mut inv, None);
    let mut fresh = inv.clone();
    patch_grid::update_surface(&mut fresh);
    if !(r.u_type == 1 && r.v_type == 1) && !capped {
        // reversed in place, the display is that of the reversed patch (to rounding: the
        // differences run the other way)
        let near = |a: &[Vector], b: &[Vector]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| dist(*p, *q) <= tol);
        ok(&mut t.inverse_display_same, in_block && near(&inv.surface0, &fresh.surface0) && near(&inv.surface1, &fresh.surface1));
    }
    patch_edit::inverse(&mut inv, None);
    if !capped {
        ok(&mut t.inverse_twice, inv.points == s.points && inv.tags == s.tags);
    }
    if let Some(c) = patch_edit::extract_spline(&s, 0, 0.5, counter) {
        // the U-running spline at v = 0.5 against the surface at its parameter
        let mut on = c.nb() > 0;
        for k in 0..nsu.min(4) {
            let u = k as f32 + 0.5;
            if let Some(q) = patch::value(&s, u, 0.5) {
                on &= dist(si_spline::eval::value(&c, u).0, q) <= tol;
            }
        }
        ok(&mut t.extract_on_surface, on);
    }
    let copy = patch::copy(&s);
    ok(&mut t.copy_same, copy.points == s.points && copy.tags == s.tags && copy.surface0 == s.surface0 && copy.surface1 == s.surface1);
}

pub fn add(t: &mut PTally, o: &PTally) {
    t.patches += o.patches;
    t.points += o.points;
    t.cells += o.cells;
    t.no_cells += o.no_cells;
    t.non_finite += o.non_finite;
    t.capped += o.capped;
    t.short_of_points += o.short_of_points;
    t.quads += o.quads;
    for (k, v) in &o.by_step {
        *t.by_step.entry(*k).or_default() += v;
    }
    macro_rules! pair { ($($f:ident),*) => { $( t.$f.0 += o.$f.0; t.$f.1 += o.$f.1; )* } }
    pair!(
        corner_is_point, wrap_same, tangent_point_is_value, tangent_matches_difference, normal_is_unit, grid_counts, grid_on_surface,
        mesh_counts, shading_counts, shading_equals_grid, shading_normals_unit, display_counts, display_on_surface, partial_update_same,
        inverse_twice, inverse_display_same, extract_on_surface, copy_same
    );
}

pub fn report(name: &str, t: &PTally) {
    println!("\n{name}: {} patches, {} control points, {} cells, {} quadrilaterals as a mesh", t.patches, t.points, t.cells, t.quads);
    println!(
        "  without a cell {}, with non-finite points {}, with a capping flag {}, file short of points {}",
        t.no_cells, t.non_finite, t.capped, t.short_of_points
    );
    let line = |label: &str, good: u64, all: u64| {
        if all != 0 {
            println!("  {label:<62} {good:>8} of {all}");
        }
    };
    line("cell corner is the control point, bit for bit (linear, Bezier)", t.corner_is_point.0, t.corner_is_point.1);
    line("closed: parameter ± a whole turn gives the same bits", t.wrap_same.0, t.wrap_same.1);
    line("point of the tangent routine is the value routine's", t.tangent_point_is_value.0, t.tangent_point_is_value.1);
    line("derivatives agree with central differences", t.tangent_matches_difference.0, t.tangent_matches_difference.1);
    line("normal is a unit vector (or a cross product below 0.0001)", t.normal_is_unit.0, t.normal_is_unit.1);
    line("grid of points: the predicted counts", t.grid_counts.0 as u64, t.grid_counts.1 as u64);
    line("grid of points: on the evaluated surface", t.grid_on_surface.0 as u64, t.grid_on_surface.1 as u64);
    line("mesh: predicted polygon count, indices inside", t.mesh_counts.0 as u64, t.mesh_counts.1 as u64);
    line("shading grid: the predicted counts", t.shading_counts.0 as u64, t.shading_counts.1 as u64);
    line("shading grid point equals the grid's, bit for bit", t.shading_equals_grid.0, t.shading_equals_grid.1);
    line("shading normal is a unit vector (or below 0.0001)", t.shading_normals_unit.0, t.shading_normals_unit.1);
    line("display curves: the predicted counts", t.display_counts.0 as u64, t.display_counts.1 as u64);
    line("display curves (U-running): on the evaluated surface", t.display_on_surface.0 as u64, t.display_on_surface.1 as u64);
    line("partial update with every point tagged = full update", t.partial_update_same.0 as u64, t.partial_update_same.1 as u64);
    line("reversed in place: display as recomputed", t.inverse_display_same.0 as u64, t.inverse_display_same.1 as u64);
    line("reversed twice: identical points and tags", t.inverse_twice.0 as u64, t.inverse_twice.1 as u64);
    line("extracted spline lies on the surface", t.extract_on_surface.0 as u64, t.extract_on_surface.1 as u64);
    line("copy: same points and display curves", t.copy_same.0 as u64, t.copy_same.1 as u64);
}
