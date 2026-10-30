// SPDX-License-Identifier: MIT
//! Checks of `triangulate.rs` (vectors of `emu_tri.py`).

use si_model::triangulate as t;
use si_x87::vectors::{Case, Check, Field, Verdict};

use crate::support::{same, same_field};

fn points(f: &Field, count: usize) -> Vec<[f32; 3]> {
    f.f32s().chunks(3).take(count).map(|p| [p[0], p[1], p[2]]).collect()
}

/// A buffer of the case after the call: its first words replaced by the port's.
fn after(f: &Field, head: &[u32]) -> Vec<u32> {
    let mut w = crate::values(f);
    w[..head.len()].copy_from_slice(head);
    w
}

fn contour(c: &Case) -> Verdict {
    let (np, ni) = (c.input("np").i32().max(0) as usize, c.input("ni").i32().max(0) as usize);
    let mut pts = points(c.input("pts"), np);
    let mut idx: Vec<i32> = c.input("idx").i32s()[..ni].to_vec();
    let (ok, tris) = t::triangulate_contour(&mut pts, &mut idx);
    let flat: Vec<u32> = tris.iter().flatten().map(|&i| i as u32).collect();
    let pw: Vec<u32> = pts.iter().flatten().map(|v| v.to_bits()).collect();
    let iw: Vec<u32> = idx.iter().map(|&i| i as u32).collect();
    same(c, "ax", &[ok as u32])
        .and(|| same(c, "r.nb", &[tris.len() as u32]))
        .and(|| same(c, "r.tri", &flat))
        .and(|| same_field(c.after("pts"), "pts", &after(c.input("pts"), &pw)))
        .and(|| same_field(c.after("idx"), "idx", &after(c.input("idx"), &iw)))
}

fn merge(c: &Case) -> Verdict {
    let pts = points(c.input("pts"), 64);
    let n = c.input("n").i32() as usize;
    let idx: Vec<i32> = c.input("idx").i32s()[..n].to_vec();
    let mut holes = c.input("holes").i32();
    let mut work: Vec<i32> = c.input("work").i32s()[..n].to_vec();
    let ok = t::merge_holes(&pts, &idx, &mut holes, &mut work);
    let ww: Vec<u32> = work.iter().map(|&i| i as u32).collect();
    let first = same(c, "ax", &[ok as u32]).and(|| same_field(c.after("holes"), "holes", &[holes as u32]));
    if !ok {
        // the original has also moved its corner count on by the merges it did make before
        // it gave up; the port's list is untouched and has no such count
        return first.and(|| same_field(c.after("work"), "work", &after(c.input("work"), &ww)));
    }
    first.and(|| same_field(c.after("n"), "n", &[work.len() as u32])).and(|| same_field(c.after("work"), "work", &after(c.input("work"), &ww)))
}

fn segment(c: &Case) -> Verdict {
    let p = |n: &str| {
        let f = c.input(n).f32s();
        [f[0], f[1], f[2]]
    };
    same(c, "ax", &[t::segment_intersect(p("a"), p("b"), p("c"), p("d")) as u32])
}

fn outline(c: &Case) -> (Vec<[f32; 3]>, Vec<i32>) {
    let n = c.input("n").i32().max(0) as usize;
    (points(c.input("pts"), 64), c.input("idx").i32s()[..n].to_vec())
}

fn concavity(c: &Case) -> Verdict {
    let (pts, idx) = outline(c);
    same(c, "eax", &[t::find_concavity_pt(&pts, &idx, c.input("start").i32()) as u32])
}

fn valid(c: &Case) -> Verdict {
    let (pts, idx) = outline(c);
    same(c, "ax", &[t::pol_valid(&pts, &idx, c.input("start").i32(), c.input("count").i32()) as u32])
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_triangulateContour" => contour,
        "mergeHoles" => merge,
        "SI_segmentIntersect" => segment,
        "findConcavityPt" => concavity,
        "polValid" => valid,
        _ => return None,
    })
}
