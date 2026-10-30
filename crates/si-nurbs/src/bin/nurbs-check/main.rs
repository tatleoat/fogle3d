// SPDX-License-Identifier: MIT
//! Runs `si-nurbs` over every NURBS curve and surface of the model files under a directory
//! (read by `si-hrc`): a census, then invariants the evaluators must meet on each. These are
//! checks of the port against itself, not comparisons with the original (that is
//! `nurbs-emucheck`).
//!
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-check re/bin/si3d40-ia/installed)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{Content, Definition, Kind, Node};
use si_nurbs::own::{self, Curve, NoHost, Surface};
use si_nurbs::{eval, insert, sample, xyzw, Statics};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "hrc") {
            out.push(p);
        }
    }
}

/// good, of
#[derive(Default, Clone, Copy)]
struct Ok2(u64, u64);

impl Ok2 {
    fn add(&mut self, good: bool) {
        self.1 += 1;
        self.0 += good as u64;
    }
}

impl std::fmt::Display for Ok2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} of {}", self.0, self.1)
    }
}

#[derive(Default)]
struct Tally {
    curves: BTreeMap<String, u32>,
    surfaces: BTreeMap<String, u32>,
    curve_points: u64,
    surface_points: u64,
    rational_curves: u32,
    rational_surfaces: u32,
    trims: u32,
    trim_curves: u32,
    projected: u32,
    // layout
    knot_count_law: Ok2,
    range_law: Ok2,
    knots_sorted: Ok2,
    grid_law: Ok2,
    // curves
    clamped_end_bits: Ok2,
    clamped_end_worst_ulp: u64,
    closed_seam_bits: Ok2,
    closed_seam_close: Ok2,
    beyond_range_is_end: Ok2,
    derivative_matches_difference: Ok2,
    insert_keeps_shape: Ok2,
    insert_worst: f64,
    insert_counts: Ok2,
    samples_count: Ok2,
    // surfaces
    value_is_value4d: Ok2,
    extracted_u_on_surface: Ok2,
    extracted_v_on_surface: Ok2,
    extracted_worst: f64,
    normal_unit: Ok2,
    normal_zero: u64,
    normal_searches: u64,
    normal_matches_difference: Ok2,
    swap_twice_identity: Ok2,
    copy_identity: Ok2,
    refused: u64,
}

fn dist(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn ulps(a: f64, b: f64) -> u64 {
    if a == b {
        return 0;
    }
    let (x, y) = (a.to_bits() as i64, b.to_bits() as i64);
    if (x < 0) != (y < 0) {
        return u64::MAX;
    }
    (x - y).unsigned_abs()
}

fn dir_label(order: i32, closed: i16) -> String {
    format!("order {order} {}", if closed != 0 { "closed" } else { "open" })
}

/// The laws of section 2 of finding 0025 on one direction.
fn layout(t: &mut Tally, d: si_nurbs::Dir) {
    let r = d.rec;
    let law = if r.closed != 0 { r.nb_points + 1 } else { r.nb_points + r.order - 2 };
    t.knot_count_law.add(r.nb_knots == law && d.knots.len() == law as usize);
    let (a, b) = if r.closed != 0 { (0, r.nb_knots - 1) } else { (r.order - 2, r.nb_knots - r.order + 1) };
    let at = |i: i32| usize::try_from(i).ok().and_then(|i| d.knots.get(i)).copied();
    t.range_law.add(at(a).map(|k| k.to_bits()) == Some(r.range_start.0.to_bits()) && at(b).map(|k| k.to_bits()) == Some(r.range_end.0.to_bits()));
    t.knots_sorted.add(d.knots.windows(2).all(|w| w[0] <= w[1]));
}

fn extent(pts: &[si_nurbs::NurbsPoint]) -> f64 {
    let mut m = 1e-9f64;
    for p in pts {
        let q = xyzw(p);
        let w = if q[3].abs() > 1e-6 { q[3] } else { 1.0 };
        for c in &q[..3] {
            m = m.max((c / w).abs());
        }
    }
    m
}

fn finite(pts: &[si_nurbs::NurbsPoint], knots: &[f64]) -> bool {
    pts.iter().all(|p| xyzw(p).iter().all(|c| c.is_finite())) && knots.iter().all(|k| k.is_finite())
}

/// Parameters strictly inside each span and on each knot of the range.
fn params(d: si_nurbs::Dir) -> Vec<f64> {
    let r = d.rec;
    let (a, b) = if r.closed != 0 { (0, r.nb_knots - 1) } else { (r.order - 2, r.nb_knots - r.order + 1) };
    let mut out = Vec::new();
    for i in a.max(0)..b.min(d.knots.len() as i32 - 1) {
        let (k0, k1) = (d.knots[i as usize], d.knots[i as usize + 1]);
        if k1 - k0 > 1e-4 {
            out.push(k0);
            out.push(k0 + 0.3 * (k1 - k0));
            out.push(k0 + 0.71 * (k1 - k0));
        }
    }
    out.push(r.range_end.0);
    out
}

fn check_curve(c: &Curve, t: &mut Tally, st: &mut Statics) {
    let r = &c.rec.dir;
    *t.curves.entry(dir_label(r.order, r.closed)).or_default() += 1;
    t.curve_points += r.nb_points.max(0) as u64;
    let v = c.view();
    let pts = v.points;
    if pts.iter().any(|p| p.w.0 != 1.0) {
        t.rational_curves += 1;
    }
    layout(t, v.dir());
    if r.nb_points < r.order || r.order < 2 || !finite(pts, v.knots) || pts.len() != r.nb_points as usize {
        t.refused += eval::crv_value(st, v, r.range_start.0, 0).is_none() as u64;
        return;
    }
    let size = extent(pts);
    let tol = size * 1e-9 + 1e-12;
    let cart = |p: &si_nurbs::NurbsPoint| -> [f64; 3] {
        let q = xyzw(p);
        let inv = 1.0 / q[3];
        [q[0] * inv, q[1] * inv, q[2] * inv]
    };
    let value = |st: &mut Statics, x: f64| eval::crv_value(st, v, x, 0);
    let (Some(p0), Some(p1)) = (value(st, r.range_start.0), value(st, r.range_end.0)) else {
        t.refused += 1;
        return;
    };
    // ends of a clamped curve are its end control points
    if r.closed == 0 {
        let k = r.order as usize;
        let n = v.knots.len();
        let clamped_start = v.knots[..k - 1].windows(2).all(|w| w[0] == w[1]);
        let clamped_end = v.knots[n + 1 - k..].windows(2).all(|w| w[0] == w[1]);
        for (clamped, got, cp) in [(clamped_start, p0, &pts[0]), (clamped_end, p1, &pts[pts.len() - 1])] {
            if clamped && cp.w.0.abs() > 1e-6 {
                let want = cart(cp);
                let worst = (0..3).map(|q| ulps(got[q], want[q])).max().unwrap();
                t.clamped_end_bits.add(worst == 0);
                t.clamped_end_worst_ulp = t.clamped_end_worst_ulp.max(worst);
            }
        }
        // beyond the range of a clamped curve every blend factor is clamped: the end's
        // value, bit for bit (an unclamped curve gives a control point there instead)
        if clamped_start && clamped_end {
            let span = r.range_end.0 - r.range_start.0;
            let below = value(st, r.range_start.0 - 0.5 * span - 1.0);
            let above = value(st, r.range_end.0 + 0.5 * span + 1.0);
            t.beyond_range_is_end.add(below.map(|p| p.map(f64::to_bits)) == Some(p0.map(f64::to_bits)) && above.map(|p| p.map(f64::to_bits)) == Some(p1.map(f64::to_bits)));
        }
    } else {
        // one period on: the seam
        t.closed_seam_bits.add(p0.map(f64::to_bits) == p1.map(f64::to_bits));
        t.closed_seam_close.add(dist(&p0, &p1) <= size * 1e-9 + 1e-12);
    }
    // first derivative against a central difference
    let span = r.range_end.0 - r.range_start.0;
    let h = span * 1e-6;
    let ps = params(v.dir());
    for &x in ps.iter().skip(1).step_by(3) {
        let (Some(d1), Some(a), Some(b)) = (eval::crv_value(st, v, x, 1), value(st, x - h), value(st, x + h)) else { continue };
        let num = [(b[0] - a[0]) / (2.0 * h), (b[1] - a[1]) / (2.0 * h), (b[2] - a[2]) / (2.0 * h)];
        let scale = dist(&d1, &[0.0; 3]).max(size / span.max(1e-9));
        t.derivative_matches_difference.add(dist(&d1, &num) <= scale * 1e-3);
    }
    // a knot inserted in the middle of every span leaves the sampled curve where it was
    let before: Vec<Option<[f64; 3]>> = ps.iter().map(|&x| value(st, x)).collect();
    let mut work = c.clone();
    let mut inserted = 0;
    let (a, b) = if r.closed != 0 { (0, r.nb_knots - 1) } else { (r.order - 2, r.nb_knots - r.order + 1) };
    let mids: Vec<f64> = (a..b).filter_map(|i| {
        let (k0, k1) = (v.knots[i as usize], v.knots[i as usize + 1]);
        (k1 - k0 > 1e-4).then_some(0.5 * (k0 + k1))
    }).collect();
    let mut fine = true;
    for m in &mids {
        match insert::crv_knot_insert(st, &mut NoHost, &mut work, *m, 1, 1) {
            Some(1) => inserted += 1,
            _ => fine = false,
        }
    }
    t.insert_counts.add(fine && work.rec.dir.nb_points == r.nb_points + inserted && work.rec.dir.nb_knots == r.nb_knots + inserted);
    let wv = work.view();
    for (x, was) in ps.iter().zip(&before) {
        let (Some(was), Some(now)) = (was, eval::crv_value(st, wv, *x, 0)) else { continue };
        let d = dist(was, &now);
        t.insert_worst = t.insert_worst.max(d / size);
        t.insert_keeps_shape.add(d <= tol.max(size * 1e-9));
    }
    // samples at a fixed step
    let step = span / 8.0;
    if let Some(Some((n, pts))) = sample::crv_samples(st, v, 0, step) {
        t.samples_count.add((n == 9 || n == 8) && pts.len() == n as usize && dist(&pts[0], &p0) == 0.0);
    }
}

fn check_surface(s: &Surface, t: &mut Tally, st: &mut Statics) {
    let rec = &s.rec;
    *t.surfaces.entry(format!("U {} × V {}", dir_label(rec.u.order, rec.u.closed), dir_label(rec.v.order, rec.v.closed))).or_default() += 1;
    t.surface_points += rec.nb_points.max(0) as u64;
    let v = s.view();
    if v.points.iter().any(|p| p.w.0 != 1.0) {
        t.rational_surfaces += 1;
    }
    t.trims += s.trims.as_ref().map_or(0, |l| l.len() as u32);
    t.trim_curves += s.trims.as_ref().map_or(0, |l| l.iter().flatten().map(|c| c.curves.as_ref().map_or(0, |v| v.len() as u32)).sum());
    t.projected += s.projected.as_ref().map_or(0, |l| l.len() as u32);
    layout(t, v.u());
    layout(t, v.v());
    let grid = rec.u.nb_points as i64 * rec.v.nb_points as i64 == rec.nb_points as i64 && v.points.len() == rec.nb_points as usize;
    t.grid_law.add(grid);
    if !grid || rec.u.order < 2 || rec.v.order < 2 || rec.u.nb_points < rec.u.order || rec.v.nb_points < rec.v.order || !finite(v.points, v.knots_u) || !finite(&[], v.knots_v) {
        t.refused += eval::srf_value(st, v, rec.u.range_start.0, rec.v.range_start.0).is_none() as u64;
        return;
    }
    let size = extent(v.points);
    let tol = size * 1e-9 + 1e-12;
    let (us, vs) = (params(v.u()), params(v.v()));
    let (su, sv) = (rec.u.range_end.0 - rec.u.range_start.0, rec.v.range_end.0 - rec.v.range_start.0);
    for (iu, &u) in us.iter().enumerate().step_by(2) {
        let cu = eval::srf_extract_curve_u(st, v, u, 0);
        for (iv, &w) in vs.iter().enumerate().step_by(2) {
            let Some(p) = eval::srf_value(st, v, u, w) else {
                t.refused += 1;
                continue;
            };
            // the homogeneous point divided is the point
            if let Some(p4) = eval::srf_value4d(st, v, u, w) {
                if p4.w.0.abs() > 1e-6 {
                    let q = [p4.x.0 / p4.w.0, p4.y.0 / p4.w.0, p4.z.0 / p4.w.0];
                    t.value_is_value4d.add(dist(&p, &q) <= tol);
                }
            }
            // the extracted curves lie on the surface
            if let Some(q) = cu.as_ref().and_then(|c| eval::crv_value(st, c.view(), w, 0)) {
                t.extracted_worst = t.extracted_worst.max(dist(&p, &q) / size);
                t.extracted_u_on_surface.add(dist(&p, &q) <= tol);
            }
            if iu % 4 == 0 {
                if let Some(q) = eval::srf_extract_curve_v(st, v, w, 0).and_then(|c| eval::crv_value(st, c.view(), u, 0)) {
                    t.extracted_worst = t.extracted_worst.max(dist(&p, &q) / size);
                    t.extracted_v_on_surface.add(dist(&p, &q) <= tol);
                }
            }
            // the normal
            let mut tries = [0u32; 2];
            if let Some((pn, n)) = eval::srf_value_and_normal_traced(st, v, u, w, None, None, &mut tries) {
                let len = dist(&n, &[0.0; 3]);
                if len == 0.0 {
                    t.normal_zero += 1;
                } else {
                    t.normal_unit.add((len - 1.0).abs() <= 1e-6);
                }
                t.normal_searches += (tries[0] + tries[1] > 0) as u64;
                // against the cross product of difference quotients, where the surface is regular
                if tries == [0, 0] && iv % 4 == 0 && len > 0.5 {
                    let (hu, hv) = (su * 1e-5, sv * 1e-5);
                    let (iu_in, iv_in) = (u - hu > rec.u.range_start.0 && u + hu < rec.u.range_end.0, w - hv > rec.v.range_start.0 && w + hv < rec.v.range_end.0);
                    if let (true, true, Some(a), Some(b), Some(c), Some(d)) =
                        (iu_in, iv_in, eval::srf_value(st, v, u - hu, w), eval::srf_value(st, v, u + hu, w), eval::srf_value(st, v, u, w - hv), eval::srf_value(st, v, u, w + hv))
                    {
                        let du = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let dv = [d[0] - c[0], d[1] - c[1], d[2] - c[2]];
                        let x = [du[1] * dv[2] - du[2] * dv[1], du[2] * dv[0] - du[0] * dv[2], du[0] * dv[1] - du[1] * dv[0]];
                        let xl = dist(&x, &[0.0; 3]);
                        if xl > 0.0 {
                            let cosine = (x[0] * n[0] + x[1] * n[1] + x[2] * n[2]) / xl;
                            t.normal_matches_difference.add(cosine > 0.999);
                        }
                    }
                }
                let _ = pn;
            }
        }
    }
    // the exchange of U and V twice is the identity; a copy equals its source
    let mut w = s.clone();
    let twice = own::srf_swap_uv(&mut NoHost, &mut w) == Some(1) && own::srf_swap_uv(&mut NoHost, &mut w) == Some(1);
    t.swap_twice_identity.add(twice && w == *s);
    if let Some(c) = own::srf_copy(&mut NoHost, s) {
        let same_points = c.points == s.points && c.knots_u == s.knots_u && c.knots_v == s.knots_v;
        let same_lists = c.trims.as_ref().map(|l| l.len()) == s.trims.as_ref().map(|l| l.len()) && c.projected.as_ref().map(|l| l.len()) == s.projected.as_ref().map(|l| l.len());
        t.copy_identity.add(same_points && same_lists && c.rec.u == s.rec.u && c.rec.v == s.rec.v);
    }
}

/// The conversion to the renderer's triangles on every surface (finding 0025, section 13):
/// counts and invariants of the port, not comparisons with the original.
#[derive(Default)]
struct Conv {
    converted: u32,
    trimmed: u32,
    halted: u32,
    vertices: u64,
    triangles: u64,
    corners_inside: u64,
    normals_unit: u64,
    normals_short: u64,
    repeated: u32,
}

/// A surface of a file laid out in a fresh image as the loader lays it out (record, knot
/// vectors, points; the trim count kept, every other list pointer null), then
/// `SI_nurbs2Triangle(srf, srf, null, 1, …, −1)` as `SI_collect` calls it.
fn convert_surface(s: &Surface, c: &mut Conv) {
    use si_mem::{Mem, Ptr32};
    let mut m = Mem::new();
    let put_f64s = |m: &mut Mem, v: &Option<Vec<f64>>| -> u32 {
        let Some(v) = v else { return 0 };
        let a = m.place((8 * v.len()).max(8) as u32, 8);
        for (i, x) in v.iter().enumerate() {
            m.set_f64(a + 8 * i as u32, *x);
        }
        a
    };
    let ku = put_f64s(&mut m, &s.knots_u);
    let kv = put_f64s(&mut m, &s.knots_v);
    let pts = match &s.points {
        Some(p) => {
            let a = m.place((0x24 * p.len()).max(4) as u32, 8);
            for (i, q) in p.iter().enumerate() {
                m.write(Ptr32::new(a + 0x24 * i as u32), *q);
            }
            a
        }
        None => 0,
    };
    let srf = m.place(0xdc, 8);
    m.write(Ptr32::new(srf), s.rec);
    m.set_u32(srf + 0x0c, ku);
    m.set_u32(srf + 0x30, kv);
    m.set_u32(srf + 0x54, pts);
    for off in [0x64, 0x68, 0x6c, 0x80, 0x84, 0xd8] {
        m.set_u32(srf + off, 0);
    }
    let out = m.place(16, 4);
    match si_nurbs::convert::nurbs2triangle(&mut m, srf, srf, 0, 1, out, out + 4, out + 8, out + 12, -1) {
        Err(si_nurbs::image::Halt::Unported(_)) => c.trimmed += 1,
        Err(_) => c.halted += 1,
        Ok(()) if m.fault().is_some() => c.halted += 1,
        Ok(()) => {
            c.converted += 1;
            let (nv, verts, nt, tris) = (m.i32(out), m.u32(out + 4), m.i32(out + 8), m.u32(out + 12));
            c.vertices += nv.max(0) as u64;
            c.triangles += nt.max(0) as u64;
            let (nu1, nv1) = (m.i32(si_nurbs::convert::U_NO_POINTS) - 1, m.i32(si_nurbs::convert::V_NO_POINTS) - 1);
            c.repeated += (nt != 2 * nu1 * nv1) as u32;
            for t in 0..nt.max(0) as u32 {
                for k in 0..3 {
                    let p = m.u32(tris + 0x60 * t + 4 * k);
                    let off = p.wrapping_sub(verts);
                    c.corners_inside += (off % 0x4c == 0 && off / 0x4c < nv as u32) as u64;
                }
            }
            for v in 0..nv.max(0) as u32 {
                let a = verts + 0x4c * v;
                let n = [m.f32(a + 0x14) as f64, m.f32(a + 0x18) as f64, m.f32(a + 0x1c) as f64];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if (len - 1.0).abs() < 1e-5 {
                    c.normals_unit += 1;
                } else if len <= 1e-4 {
                    c.normals_short += 1;
                }
            }
        }
    }
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: nurbs-check <directory>");
        std::process::exit(2);
    };
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let mut t = Tally::default();
    let mut conv = Conv::default();
    let (mut read, mut with_nurbs, mut panics) = (0u32, 0u32, 0u32);
    let (mut from_nodes, mut from_faces, mut from_trims) = (0u32, 0u32, 0u32);
    let hook = std::panic::take_hook();
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        let Some(file) = std::panic::catch_unwind(|| si_hrc::read_file(&data, Kind::Model)).ok().flatten() else { continue };
        let Content::Model(root) = file.content else { continue };
        read += 1;
        let mut curves: Vec<Curve> = Vec::new();
        let mut surfaces: Vec<Surface> = Vec::new();
        let mut stack: Vec<&Node> = vec![&root];
        while let Some(n) = stack.pop() {
            for d in &n.definitions {
                match d {
                    Definition::NurbsCurve(c) => {
                        curves.push(Curve::from_file(c));
                        from_nodes += 1;
                    }
                    Definition::NurbsSurface(s) => {
                        let s = Surface::from_file(s);
                        for l in [&s.trims, &s.projected].into_iter().flatten() {
                            for comp in l.iter().flatten() {
                                for c in comp.curves.iter().flatten().flatten() {
                                    curves.push((**c).clone());
                                    from_trims += 1;
                                }
                            }
                        }
                        surfaces.push(s);
                    }
                    Definition::Face(f) if f.is_spline == 0 => {
                        from_faces += f.nurbs.len() as u32;
                        curves.extend(f.nurbs.iter().map(Curve::from_file));
                    }
                    _ => {}
                }
            }
            if let Some(b) = &n.brother {
                stack.push(b);
            }
            if let Some(s) = &n.son {
                stack.push(s);
            }
        }
        with_nurbs += (!curves.is_empty() || !surfaces.is_empty()) as u32;
        let mut st = Statics::default();
        std::panic::set_hook(Box::new(|_| {}));
        for c in &curves {
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_curve(c, &mut t, &mut st)));
            panics += done.is_err() as u32;
        }
        for s in &surfaces {
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| convert_surface(s, &mut conv)));
            if done.is_err() {
                panics += 1;
                eprintln!("panic converting a surface of {}", p.display());
            }
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_surface(s, &mut t, &mut st)));
            if done.is_err() {
                panics += 1;
                eprintln!("panic on a surface of {}", p.display());
            }
        }
        std::panic::set_hook(Box::new(|_| {}));
    }
    std::panic::set_hook(hook);

    println!("model files read: {read}; with NURBS: {with_nurbs}; panics: {panics}");
    println!("curves: {} ({from_nodes} curve nodes, {from_faces} of faces, {from_trims} of trims and projected curves), {} control points, {} rational", t.curves.values().sum::<u32>(), t.curve_points, t.rational_curves);
    for (k, n) in &t.curves {
        println!("  {k}: {n}");
    }
    println!("surfaces: {}, {} control points, {} rational; {} trims of {} curves, {} projected curves", t.surfaces.values().sum::<u32>(), t.surface_points, t.rational_surfaces, t.trims, t.trim_curves, t.projected);
    for (k, n) in &t.surfaces {
        println!("  {k}: {n}");
    }
    println!("layout, per direction");
    println!("  knot count is points + order - 2 (open) or points + 1 (closed): {}", t.knot_count_law);
    println!("  range is knots[order - 2] .. knots[count - order + 1] (open), first .. last (closed), bit for bit: {}", t.range_law);
    println!("  knots do not decrease: {}", t.knots_sorted);
    println!("  surface point count is nu x nv: {}", t.grid_law);
    println!("curves");
    println!("  clamped end: value equals the end control point bit for bit: {} (worst {} units in the last place)", t.clamped_end_bits, t.clamped_end_worst_ulp);
    println!("  beyond the range of a clamped curve: the end's value bit for bit: {}", t.beyond_range_is_end);
    println!("  closed: value at range end equals value at range start bit for bit: {}; to 1e-9 of the size: {}", t.closed_seam_bits, t.closed_seam_close);
    println!("  first derivative agrees with a central difference: {}", t.derivative_matches_difference);
    println!("  a knot inserted in every span: counts grow by the insertions: {}", t.insert_counts);
    println!("  ... and the sampled curve stays within 1e-9 of its size: {} (worst {:.3e} of the size)", t.insert_keeps_shape, t.insert_worst);
    println!("  samples at an eighth of the range: 8 or 9 points, the first the start: {}", t.samples_count);
    println!("surfaces");
    println!("  point equals homogeneous point divided, to 1e-9 of the size: {}", t.value_is_value4d);
    println!("  point of the extracted curve in V at u is the surface point, to 1e-9: {}", t.extracted_u_on_surface);
    println!("  point of the extracted curve in U at v is the surface point, to 1e-9: {} (worst of both {:.3e} of the size)", t.extracted_v_on_surface, t.extracted_worst);
    println!("  normal is a unit vector to 1e-6: {}; zero normals: {}; points that needed the search: {}", t.normal_unit, t.normal_zero, t.normal_searches);
    println!("  normal within 2.6 degrees of the cross product of difference quotients: {}", t.normal_matches_difference);
    println!("  U and V exchanged twice: the same surface: {}", t.swap_twice_identity);
    println!("  a copy has the same directions, knots, points and list lengths: {}", t.copy_identity);
    println!("inputs the evaluator refuses (too few points, order below 2, non-finite): {}", t.refused);
    println!("conversion to the renderer's triangles (SI_nurbs2Triangle on an image, flag -1)");
    println!("  converted {}, trimmed (TriangulateTrimmedNurbsSrf, not ported) {}, halted {}", conv.converted, conv.trimmed, conv.halted);
    println!("  vertices {}, triangles {}; surfaces with a repeated sample (a crease): {}", conv.vertices, conv.triangles, conv.repeated);
    println!("  triangle corners that are vertices of the block: {} of {}", conv.corners_inside, 3 * conv.triangles);
    println!("  normals of length 1 to 1e-5: {}; left undivided (length not above 1e-4): {}; others: {}", conv.normals_unit, conv.normals_short, conv.vertices - conv.normals_unit - conv.normals_short);
    if panics > 0 {
        std::process::exit(1);
    }
}
