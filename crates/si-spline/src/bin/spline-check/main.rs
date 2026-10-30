// SPDX-License-Identifier: MIT
//! Runs the spline and patch code over every spline and patch of the sample models: no original function to
//! compare with here, so the checks are the port against itself (invariants) and "no panic".
//!
//!   (ulimit -v 2000000; timeout 600 target/si-spline/release/spline-check re/bin/si3d40-ia/installed)
//!
//! Splines are the definitions of spline nodes and the outlines and holes of faces
//! (`si-hrc` reads the files; finding 0004). Counts are per type and open or closed.

mod patches;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{Content, Definition, Kind, Node};
use si_spline::{convert, edit, eval, length, Curve, NoHost, Vector};

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

#[derive(Default)]
struct Tally {
    splines: u32,
    keys: u64,
    segments: u64,
    from_spline_nodes: u32,
    from_faces: u32,
    not_loadable: u32,
    old_layout: u32,
    no_segments: u32,
    // evaluation
    values: u64,
    join_is_key: (u64, u64),
    wrap_same: (u64, u64),
    end_clamped: (u32, u32),
    tangent_matches_difference: (u64, u64),
    // length
    table_monotone: (u32, u32),
    length_is_sum_of_parts: (u32, u32),
    length_matches_evaluation: (u32, u32),
    distance_round_trip: (u64, u64),
    // conversion and editing
    to_bezier_same_shape: (u32, u32),
    expand_shrink_identity: (u32, u32),
    inverse_twice_identity: (u32, u32),
    regular_point_count: (u32, u32),
    regular_points_on_curve: (u32, u32),
    transform_then_back: (u32, u32),
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

fn finite(c: &Curve) -> bool {
    c.used().iter().all(|k| [k.prev, k.pos, k.next].iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()))
}

/// The size of the spline, for tolerances that follow its scale.
fn extent(c: &Curve) -> f64 {
    let mut m = 1e-6f64;
    for k in c.used() {
        for v in [k.prev, k.pos, k.next] {
            m = m.max(v.x.abs() as f64).max(v.y.abs() as f64).max(v.z.abs() as f64);
        }
    }
    m
}

fn check(c: &Curve, t: &mut Tally, counter: &mut i32) {
    let host = &mut NoHost;
    t.splines += 1;
    t.keys += c.nb() as u64;
    let nseg = eval::nb_segment(c);
    if nseg <= 0 || !finite(c) {
        t.no_segments += 1;
        let _ = eval::value(c, 0.5);
        let _ = eval::tangent(c, 0.5);
        let mut d = c.clone();
        let _ = length::length(&mut d, host);
        return;
    }
    t.segments += nseg as u64;
    let size = extent(c);
    let tol = size * 2e-4;
    let closed = c.rec.closed != 0;

    // ---- evaluation: joins, ends, wrap, tangent against a difference quotient
    for s in 0..=nseg {
        let (p, _) = eval::value(c, s as f32);
        t.values += 1;
        if c.rec.kind < 2 && s < c.nb() as i32 {
            // a linear or Bézier curve passes through its keys, to the last bit
            let k = c.keys[s as usize].pos;
            ok64(&mut t.join_is_key, (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()) == (k.x.to_bits(), k.y.to_bits(), k.z.to_bits()));
        }
        if s < nseg {
            let u = s as f32 + 0.5;
            let (m, _) = eval::value(c, u);
            t.values += 1;
            if closed {
                let (w, _) = eval::value(c, u + nseg as f32);
                let (v, _) = eval::value(c, u - nseg as f32);
                ok64(&mut t.wrap_same, m == w && m == v);
            }
            if let Some(tan) = eval::tangent(c, u) {
                let h = 1.0f32 / 64.0;
                let (a, b) = (eval::value(c, u - h).0, eval::value(c, u + h).0);
                let q = Vector { x: (b.x - a.x) / (2.0 * h), y: (b.y - a.y) / (2.0 * h), z: (b.z - a.z) / (2.0 * h) };
                // a cubic's central difference is off by h² × (third derivative) / 6
                ok64(&mut t.tangent_matches_difference, dist(tan, q) <= size * 0.02 + 1e-5);
            }
            let _ = eval::acceleration(c, u);
        }
    }
    if !closed {
        let (lo, hi) = (eval::value(c, -5.0).0, eval::value(c, nseg as f32 + 7.0).0);
        ok(&mut t.end_clamped, lo == eval::value(c, 0.0).0 && hi == eval::value(c, nseg as f32).0);
    }

    // ---- length
    let mut l = c.clone();
    l.rec.phantom = 0; // the table of the curve as stored, without recomputing phantom points
    length::update_length(&mut l, 100, 0, host);
    let samples = l.rec.nb_lengths as usize;
    let table = l.lengths.clone();
    ok(&mut t.table_monotone, table.windows(2).all(|w| w[1] >= w[0]) && table.len() == samples * nseg as usize + 1);
    let whole = length::length(&mut l, host) as f64;
    // the whole is the sum of the per-segment parts the table holds
    let parts: f64 = (0..nseg as usize).map(|s| (table[(s + 1) * samples] - table[s * samples]) as f64).sum();
    ok(&mut t.length_is_sum_of_parts, (parts - whole).abs() <= whole.abs() * 1e-5 + 1e-9);
    // … and close to the polyline through directly evaluated points
    let mut direct = 0.0f64;
    let mut prev = eval::value(c, 0.0).0;
    for i in 1..=(nseg as usize * samples) {
        let p = eval::value(&l, i as f32 / samples as f32).0;
        let p = if i == nseg as usize * samples && closed { eval::value(&l, 0.0).0 } else { p };
        direct += dist(prev, p);
        prev = p;
    }
    ok(&mut t.length_matches_evaluation, (direct - whole).abs() <= whole.abs() * 2e-3 + tol);
    if whole > 1e-3 {
        for i in 0..=8 {
            let u = nseg as f32 * i as f32 / 8.0;
            let d = length::factor_to_distance(&mut l, u, host).to_f32();
            let back = length::distance_to_factor(&mut l, d, host).to_f32();
            let d2 = length::factor_to_distance(&mut l, back, host).to_f32();
            ok64(&mut t.distance_round_trip, (d - d2).abs() as f64 <= whole * 1e-4 + 2e-4);
        }
    }

    // ---- conversions
    if let Some(b) = convert::convert_to_bezier(c, counter) {
        let mut same = eval::nb_segment(&b) == nseg;
        for s in 0..nseg.min(64) {
            for u in [0.0f32, 0.25, 0.5, 0.875] {
                same &= dist(eval::value(c, s as f32 + u).0, eval::value(&b, s as f32 + u).0) <= tol * 4.0;
            }
        }
        ok(&mut t.to_bezier_same_shape, same);
        // control points out and back in: the same keys (an open spline's two outer
        // handles are recomputed by the way back, so they are compared after one round)
        if let Some(e) = convert::bezier_expand(&b, counter) {
            if let Some(s1) = convert::bezier_shrink(&e, counter) {
                let again = convert::bezier_expand(&s1, counter).and_then(|e2| convert::bezier_shrink(&e2, counter));
                // a straight segment's two control points are put on the chord by the way out,
                // and an open spline's two outer handles are recomputed by the way back
                let n = b.nb();
                let first = s1.nb() == n
                    && (0..n).all(|i| {
                        let (a, k) = (s1.keys[i], b.keys[i]);
                        let to = if i == n - 1 { 0 } else { i + 1 };
                        a.pos == k.pos
                            && (b.keys[to].segment == 2 || (!closed && i == n - 1) || a.next == k.next)
                            && (k.segment == 2 || (!closed && i == 0) || a.prev == k.prev)
                    });
                let same = again.is_some_and(|s2| {
                    s2.nb() == s1.nb() && s1.used().iter().zip(s2.used()).all(|(a, b)| (a.prev, a.pos, a.next) == (b.prev, b.pos, b.next))
                });
                ok(&mut t.expand_shrink_identity, same && first);
            }
        }
    }
    let mut inv = c.clone();
    edit::inverse(&mut inv, host);
    edit::inverse(&mut inv, host);
    ok(&mut t.inverse_twice_identity, inv.used() == c.used());

    let steps = 4;
    if let Some(lin) = convert::convert_to_linear_regular(c, steps, counter) {
        let want = if closed { steps * nseg } else { steps * nseg + 1 };
        ok(&mut t.regular_point_count, lin.nb() as i32 == want);
        let mut on = true;
        for (i, k) in lin.used().iter().enumerate() {
            let u = (i + closed as usize) as f32 / steps as f32;
            on &= dist(k.pos, eval::value(c, u).0) <= tol * 4.0;
        }
        ok(&mut t.regular_points_on_curve, on);
    }
    // a translation there and back leaves every key where it was, to rounding
    let mut moved = c.clone();
    let mut m = si_math::types::IDENTITY;
    m[3] = [1.0, -2.0, 0.5, 1.0];
    edit::transform(&mut moved, &m);
    m[3] = [-1.0, 2.0, -0.5, 1.0];
    edit::multiply_definition(&mut moved, &m, host);
    ok(&mut t.transform_then_back, moved.used().iter().zip(c.used()).all(|(a, b)| dist(a.pos, b.pos) <= size * 1e-5 + 1e-5));
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: spline-check <directory>");
        std::process::exit(2);
    };
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut ptallies: BTreeMap<String, patches::PTally> = BTreeMap::new();
    let (mut read, mut with_splines, mut with_patches, mut panics) = (0u32, 0u32, 0u32, 0u32);
    let mut counter = 0i32;
    let hook = std::panic::take_hook();
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        let Some(file) = std::panic::catch_unwind(|| si_hrc::read_file(&data, Kind::Model)).ok().flatten() else { continue };
        let Content::Model(root) = file.content else { continue };
        read += 1;
        // every spline of the file, with where it came from, and every patch
        let mut found_patches: Vec<si_hrc::Patch> = Vec::new();
        let mut found: Vec<(si_hrc::Spline, bool)> = Vec::new();
        let mut stack: Vec<&Node> = vec![&root];
        while let Some(n) = stack.pop() {
            for d in &n.definitions {
                match d {
                    Definition::Spline(s) => found.push((s.clone(), false)),
                    Definition::Patch(q) => found_patches.push(q.clone()),
                    Definition::Face(f) if f.is_spline != 0 => {
                        found.push((f.exterior.clone(), true));
                        found.extend(f.holes.iter().map(|h| (h.clone(), true)));
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
        with_splines += !found.is_empty() as u32;
        with_patches += !found_patches.is_empty() as u32;
        for q in found_patches {
            let t = ptallies.entry(patches::label(&q)).or_default();
            std::panic::set_hook(Box::new(|_| {}));
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut local = patches::PTally::default();
                let mut n = counter;
                patches::check(&q, &mut local, &mut n);
                (local, n)
            }));
            match done {
                Ok((local, n)) => {
                    counter = n;
                    patches::add(t, &local);
                }
                Err(_) => {
                    panics += 1;
                    eprintln!("PANIC (patch) in {}", p.display());
                }
            }
        }
        for (s, face) in found {
            let label = format!(
                "{} {}",
                match s.kind {
                    0 => "linear",
                    1 => "Bezier",
                    2 => "B-spline",
                    3 => "cardinal",
                    _ => "other",
                },
                if s.close != 0 { "closed" } else { "open" }
            );
            let t = tallies.entry(label).or_default();
            t.old_layout += s.old_points as u32;
            if face {
                t.from_faces += 1;
            } else {
                t.from_spline_nodes += 1;
            }
            std::panic::set_hook(Box::new(|_| {}));
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut local = Tally::default();
                let mut n = counter;
                match Curve::from_file(&s, &mut n, &mut NoHost) {
                    Some(c) => check(&c, &mut local, &mut n),
                    None => local.not_loadable += 1,
                }
                (local, n)
            }));
            std::panic::set_hook(Box::new(|_| {}));
            match done {
                Ok((local, n)) => {
                    counter = n;
                    add(t, &local);
                }
                Err(_) => {
                    panics += 1;
                    eprintln!("PANIC in {}", p.display());
                }
            }
        }
    }
    std::panic::set_hook(hook);
    println!("{} model files read, {} with splines, {} panics", read, with_splines, panics);
    let mut total = Tally::default();
    for (name, t) in &tallies {
        report(name, t);
        add(&mut total, t);
    }
    report("ALL", &total);
    println!("\n{} model files with patches", with_patches);
    let mut ptotal = patches::PTally::default();
    for (name, t) in &ptallies {
        println!("  {:<40} {:>5} patches", name, t.patches);
        patches::add(&mut ptotal, t);
    }
    let steps: Vec<String> = ptotal.by_step.iter().map(|((a, b), n)| format!("{a}x{b}: {n}")).collect();
    println!("  steps (U x V: patches): {}", steps.join(", "));
    patches::report("ALL PATCHES", &ptotal);
    std::process::exit((panics != 0) as i32);
}

fn add(t: &mut Tally, o: &Tally) {
    t.splines += o.splines;
    t.keys += o.keys;
    t.segments += o.segments;
    t.from_spline_nodes += o.from_spline_nodes;
    t.from_faces += o.from_faces;
    t.not_loadable += o.not_loadable;
    t.old_layout += o.old_layout;
    t.no_segments += o.no_segments;
    t.values += o.values;
    macro_rules! pair { ($($f:ident),*) => { $( t.$f.0 += o.$f.0; t.$f.1 += o.$f.1; )* } }
    pair!(
        join_is_key, wrap_same, end_clamped, tangent_matches_difference, table_monotone, length_is_sum_of_parts,
        length_matches_evaluation, distance_round_trip, to_bezier_same_shape, expand_shrink_identity, inverse_twice_identity,
        regular_point_count, regular_points_on_curve, transform_then_back
    );
}

fn report(name: &str, t: &Tally) {
    println!("\n{name}: {} splines ({} spline nodes, {} of faces), {} keys, {} segments", t.splines, t.from_spline_nodes, t.from_faces, t.keys, t.segments);
    println!("  without segments or with non-finite keys {}, not loadable {}, stored as points only {}, points evaluated {}", t.no_segments, t.not_loadable, t.old_layout, t.values);
    let line = |label: &str, good: u64, all: u64| {
        if all != 0 {
            println!("  {label:<58} {good:>8} of {all}");
        }
    };
    line("value at a join is the key, bit for bit (linear, Bezier)", t.join_is_key.0, t.join_is_key.1);
    line("closed: parameter ± a whole turn gives the same bits", t.wrap_same.0, t.wrap_same.1);
    line("open: parameters beyond the ends give the end points", t.end_clamped.0 as u64, t.end_clamped.1 as u64);
    line("tangent agrees with a central difference", t.tangent_matches_difference.0, t.tangent_matches_difference.1);
    line("length table non-decreasing and of the right size", t.table_monotone.0 as u64, t.table_monotone.1 as u64);
    line("length equals the sum of its segments' parts", t.length_is_sum_of_parts.0 as u64, t.length_is_sum_of_parts.1 as u64);
    line("length within 0.2% of the directly evaluated polyline", t.length_matches_evaluation.0 as u64, t.length_matches_evaluation.1 as u64);
    line("parameter -> distance -> parameter -> distance", t.distance_round_trip.0, t.distance_round_trip.1);
    line("converted to Bezier: same points", t.to_bezier_same_shape.0 as u64, t.to_bezier_same_shape.1 as u64);
    line("Bezier expanded and shrunk: stable keys", t.expand_shrink_identity.0 as u64, t.expand_shrink_identity.1 as u64);
    line("reversed twice: identical keys", t.inverse_twice_identity.0 as u64, t.inverse_twice_identity.1 as u64);
    line("regular steps: right number of points", t.regular_point_count.0 as u64, t.regular_point_count.1 as u64);
    line("regular steps: points on the curve", t.regular_points_on_curve.0 as u64, t.regular_points_on_curve.1 as u64);
    line("translated and back: keys where they were", t.transform_then_back.0 as u64, t.transform_then_back.1 as u64);
}
