// SPDX-License-Identifier: MIT
//! Triangles from a polygon: the original's `triangulate.c` (finding 0015).
//!
//! [`triangulate_contour`] is what every consumer of triangles in the program calls (the
//! renderer's `SI_meshTriangles`, the modelling command `SI_meshTriangulate`, the boolean
//! and ray-picking code, the game exporters): given the corners of one polygon **already
//! turned into the XY plane** (the callers send them through `SI_matrixZAlign` of the
//! polygon's normal; z is ignored here) it
//!
//! 1. rounds the coordinates to a grid whose step depends on their size;
//! 2. drops corners that repeat the corner before them;
//! 3. joins every hole to the outline by a bridge ([`merge_holes`]);
//! 4. cuts ears off the single outline that is left, in a fixed order.
//!
//! [`polygon_points`] prepares one polygon of a mesh the way `SI_meshTriangulate` does.
//!
//! Exactness: every test is a difference of two products of differences of float
//! coordinates, formed in the x87 register and compared there with a double tolerance.
//! Nothing calls the C library.

use si_math::algebra::matrix_z_align;
use si_math::types::{Matrix, Vector};
use si_x87::{x, Round, F80};

use crate::mesh::Mesh;

/// The tolerance of [`segment_intersect`]'s side tests: the doubles ∓0.0001 (0x086625b8,
/// 0x086625c0).
const SIDE_EPS: f64 = 1e-4;
/// The tolerance of the ear tests of [`triangulate_contour`]: the doubles 1e-7 and −1e-7
/// (0x086625a8, 0x086625b0).
const EAR_EPS: f64 = 1e-7;
/// The tolerance of the bridge tests of [`merge_holes`]: the double 0.0001 (0x08662570).
const BRIDGE_EPS: f64 = 1e-4;
/// The start values of a contour's bounding box: the floats 1e30 and −1e30 (0x08662568,
/// 0x0866256c).
const BOX_MIN: f32 = 1.0e30;
const BOX_MAX: f32 = -1.0e30;

type P = [f32; 3];

/// A point by its number; zeros for a number outside the array (the original reads
/// whatever memory lies there, a separator's −1 among them).
fn pt(points: &[P], i: i32) -> P {
    points.get(i as usize).copied().unwrap_or([0.0; 3])
}

/// (b − o) × (c − o) in the plane, as the routines of this file write it:
/// `(c.y − o.y)·(b.x − o.x) − (c.x − o.x)·(b.y − o.y)`, all in the register.
fn cross(o: P, b: P, c: P) -> F80 {
    (x(c[1]) - o[1]) * (x(b[0]) - o[0]) - (x(c[0]) - o[0]) * (x(b[1]) - o[1])
}

/// −1, 0 or 1 for a value below −0.0001, within, or above 0.0001.
fn side(v: F80) -> i32 {
    if v.lt(x(-SIDE_EPS)) {
        -1
    } else {
        v.gt(x(SIDE_EPS)) as i32
    }
}

/// Whether the segments a–b and c–d cross [soft 0x0851e11c SI_segmentIntersect]. In the XY
/// plane only. Class: model.
///
/// False when their bounding boxes do not overlap (strict comparisons of the float
/// coordinates). Otherwise s₁, s₂ = the sides of a and b with respect to c–d and s₃, s₄
/// those of c and d with respect to a–b, each −1, 0 or 1 by the cross product against
/// ∓0.0001. True when s₁ ≠ s₂ and s₃ ≠ s₄ and at least one of the two pairs is strictly
/// opposite: a proper crossing, or one segment ending on the other while the other's ends
/// lie strictly apart. Two segments that only touch end to end (both pairs hold a zero) do
/// not cross; neither do collinear ones.
///
/// (The original leaves each cross product in the float global `mD0`, 0x08822450: see
/// [`take_md0`].)
pub fn segment_intersect(a: P, b: P, c: P, d: P) -> bool {
    for k in 0..2 {
        // the larger and the smaller of each pair; with a NaN the first operand is "larger"
        let (hi1, lo1) = if x(b[k]).gt(x(a[k])) { (b[k], a[k]) } else { (a[k], b[k]) };
        let (hi2, lo2) = if x(d[k]).gt(x(c[k])) { (d[k], c[k]) } else { (c[k], d[k]) };
        if x(hi2).lt(x(lo1)) || x(lo2).gt(x(hi1)) {
            return false;
        }
    }
    let s1 = side(md0((x(a[1]) - c[1]) * (x(d[0]) - c[0]) - (x(a[0]) - c[0]) * (x(d[1]) - c[1])));
    let s2 = side(md0((x(b[1]) - c[1]) * (x(d[0]) - c[0]) - (x(b[0]) - c[0]) * (x(d[1]) - c[1])));
    if s1 == s2 {
        return false;
    }
    let s3 = side(md0((x(c[1]) - a[1]) * (x(b[0]) - a[0]) - (x(c[0]) - a[0]) * (x(b[1]) - a[1])));
    let s4 = side(md0((x(d[1]) - a[1]) * (x(b[0]) - a[0]) - (x(d[0]) - a[0]) * (x(b[1]) - a[1])));
    s3 != s4 && (s1 == -s2 || s3 == -s4)
}

thread_local! {
    /// What the original leaves in its global `mD0` (0x08822450): each cross product of
    /// [`segment_intersect`], rounded to float (`fsts`; the side is taken from the register).
    /// `None` until one is computed; [`take_md0`] reads and clears it.
    static MD0: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

fn md0(v: F80) -> F80 {
    MD0.with(|c| c.set(Some(v.to_f32())));
    v
}

/// The last value [`segment_intersect`] stored into `mD0` since the last call of this
/// function, if any (the image form stores it into the global).
pub fn take_md0() -> Option<f32> {
    MD0.with(|c| c.take())
}

/// One contour of the index list while holes are merged: where it starts and ends, and the
/// bounding box of its points (32 bytes in the original: two indices and two vectors).
#[derive(Clone, Copy, Debug)]
struct Contour {
    start: i32,
    end: i32,
    min: [f32; 2],
    max: [f32; 2],
}

impl Contour {
    fn new(start: i32) -> Contour {
        Contour { start, end: 0, min: [BOX_MIN; 2], max: [BOX_MAX; 2] }
    }
}

/// The contours of an index list: a −1 in `separators` ends one and starts the next; the
/// points are taken from `list`. (The original's first scan reads the separators from the
/// caller's array and the points from its copy; they hold the same then.)
fn contours(points: &[P], list: &[i32], separators: &[i32], count: usize) -> Vec<Contour> {
    let mut out = vec![Contour::new(0)];
    for k in 0..list.len() {
        if separators[k] == -1 {
            out.last_mut().unwrap().end = k as i32 - 1;
            out.push(Contour::new(k as i32 + 1));
        } else {
            let q = pt(points, list[k]);
            let c = out.last_mut().unwrap();
            for a in 0..2 {
                if x(q[a]).lt(x(c.min[a])) {
                    c.min[a] = q[a];
                }
                if x(q[a]).gt(x(c.max[a])) {
                    c.max[a] = q[a];
                }
            }
        }
    }
    // the table has room for the holes counted by the caller, plus one
    out.resize(out.len().max(count + 1), Contour::new(0));
    out
}

/// Joins every hole to the outline [soft 0x0851d030 mergeHoles]. `work` is the index list
/// (contour, −1, contour, …; the first contour is the outline), `holes` the number of
/// separators. On success `work` is one contour without separators and `holes` is 0; on
/// failure (no bridge can be found for the holes that are left) the answer is false,
/// `work` is as it was and `holes` says how many were not merged. Class: model.
///
/// Repeatedly, the first triple (hole h, outline corner o, hole corner k), **in the order
/// holes, then outline corners, then hole corners**, is taken for which
///
/// - the hole corner H lies in the angle of the outline at its corner A: of the three
///   cross products (H against the edge before A, H against the edge after A, the turn at
///   A itself) at least two are above 0.0001;
/// - likewise A lies in the angle of the hole at H (the hole's own direction of travel);
/// - the bridge A–H crosses no edge of any contour ([`segment_intersect`]). For the
///   outline and the hole itself every edge is tested; for another hole only if the bridge
///   crosses one of the two diagonals of its bounding box.
///
/// The list then becomes: the outline up to and including A, the hole from H to its end,
/// the hole from its start to H (H again), the outline from A (again) to its end, and the
/// other holes behind, each after a −1. So each merge adds two corners and removes one
/// separator.
///
/// The original leaks its three work arrays when it fails.
pub fn merge_holes(points: &[P], separators: &[i32], holes: &mut i32, work: &mut Vec<i32>) -> bool {
    let mut cur: Vec<i32> = work.clone();
    let mut ct = contours(points, &cur, separators, (*holes).max(0) as usize);
    let mut last = cur.iter().zip(separators).filter(|(_, s)| **s == -1).count();
    loop {
        if let Some(c) = ct.get_mut(last) {
            c.end = cur.len() as i32 - 1;
        }
        if *holes == 0 {
            *work = cur;
            return true;
        }
        let Some((h, o, k)) = find_bridge(points, &cur, &ct, *holes) else {
            return false;
        };
        let (outer, hole) = (ct[0], ct[h]);
        let mut next = Vec::with_capacity(cur.len() + 1);
        next.extend_from_slice(&cur[..=o as usize]);
        next.extend_from_slice(&cur[k as usize..=hole.end as usize]);
        next.extend_from_slice(&cur[hole.start as usize..=k as usize]);
        next.extend_from_slice(&cur[o as usize..=outer.end as usize]);
        for j in 1..=*holes as usize {
            if j != h {
                next.push(-1);
                if ct[j].start <= ct[j].end {
                    next.extend_from_slice(&cur[ct[j].start as usize..=ct[j].end as usize]);
                }
            }
        }
        cur = next;
        *holes -= 1;
        ct = contours(points, &cur, &cur, (*holes).max(0) as usize);
        last = cur.iter().filter(|&&i| i == -1).count();
    }
}

/// The first acceptable bridge of [`merge_holes`]: (hole, outline position, hole position).
fn find_bridge(points: &[P], cur: &[i32], ct: &[Contour], holes: i32) -> Option<(usize, i32, i32)> {
    let at = |i: i32| pt(points, cur.get(i as usize).copied().unwrap_or(-1));
    let outer = ct[0];
    if outer.end < 0 {
        return None;
    }
    for h in 1..=holes as usize {
        let hole = ct[h];
        for o in 0..=outer.end {
            let p = at(if o == 0 { outer.end } else { o - 1 });
            let a = at(o);
            let n = at(if o == outer.end { 0 } else { o + 1 });
            'corner: for k in hole.start..=hole.end {
                let hp = at(k);
                let c1 = (x(hp[1]) - p[1]) * (x(a[0]) - p[0]) - (x(hp[0]) - p[0]) * (x(a[1]) - p[1]);
                let c2 = (x(hp[1]) - a[1]) * (x(n[0]) - a[0]) - (x(hp[0]) - a[0]) * (x(n[1]) - a[1]);
                let c3 = (x(a[1]) - p[1]) * (x(n[0]) - p[0]) - (x(a[0]) - p[0]) * (x(n[1]) - p[1]);
                let eps = x(BRIDGE_EPS);
                if (eps.lt(c1) as i32 + eps.lt(c2) as i32 + eps.lt(c3) as i32) < 2 {
                    continue;
                }
                let before = at(if k == hole.start { hole.end } else { k - 1 });
                let after = at(if k == hole.end { hole.start } else { k + 1 });
                // one of the two differences is spilled to a float before it is used
                let dx = (x(hp[0]) - before[0]).to_f32();
                let dy = x(hp[1]) - before[1];
                let d1 = (x(a[1]) - before[1]) * dx - (x(a[0]) - before[0]) * dy;
                let d2 = (x(a[1]) - hp[1]) * (x(after[0]) - hp[0]) - (x(a[0]) - hp[0]) * (x(after[1]) - hp[1]);
                let d3 = dy * (x(after[0]) - before[0]) - (x(after[1]) - before[1]) * dx;
                if (eps.lt(d1) as i32 + eps.lt(d2) as i32 + d3.gt(eps) as i32) < 2 {
                    continue;
                }
                for (j, c) in ct.iter().enumerate().take(holes as usize + 1) {
                    if j != 0 && j != h {
                        let (lo, hi) = ([c.min[0], c.min[1], 0.0], [c.max[0], c.max[1], 0.0]);
                        if !segment_intersect(a, hp, lo, hi) && !segment_intersect(a, hp, [c.min[0], c.max[1], 0.0], [c.max[0], c.min[1], 0.0]) {
                            continue;
                        }
                    }
                    for m in c.start..c.end {
                        if segment_intersect(a, hp, at(m), at(m + 1)) {
                            continue 'corner;
                        }
                    }
                    if segment_intersect(a, hp, at(c.start), at(c.end)) {
                        continue 'corner;
                    }
                }
                return Some((h, o, k));
            }
        }
    }
    None
}

/// Rounds one coordinate towards zero on the grid of its size: steps of 1e-6 below 1,
/// of 1e-4 below 100, of 0.01 below 10,000; larger values and NaN stay as they are.
/// r32(trunc(v × f) / d) with the product in the register, `f` a float and `d` a double.
fn snap(v: f32) -> f32 {
    let a = x(v).abs();
    let (f, d) = if a.lt(x(1.0f32)) {
        (1_000_000.0f32, 1_000_000.0f64)
    } else if a.lt(x(100.0f64)) {
        (10_000.0, 10_000.0)
    } else if a.lt(x(10_000.0f64)) {
        (100.0, 100.0)
    } else {
        return v;
    };
    (x((x(v) * f).to_i32(Round::Zero)) / x(d)).to_f32()
}

/// The triangles of one polygon [soft 0x0851dadc SI_triangulateContour]. Class: model.
///
/// `points` are the polygon's points in its own plane (x, y; z is carried along),
/// `indices` its corners as numbers of points, with −1 between contours: the outline
/// first, then the holes. Both are **changed**: every coordinate of every point is rounded
/// ([`snap`]: towards zero, to 1e-6 below 1, 1e-4 below 100, 0.01 below 10,000), and the
/// index list is compacted in place where a corner repeats the one before it (the first
/// against the last). The answer is whether the polygon could be triangulated and the
/// triangles as triples of point numbers; false only when a hole could not be joined
/// ([`merge_holes`]).
///
/// **The method**, on the list of n corners that is left, with `i` starting at 0 and
/// `last` = 0. While n > 2, for the corners P, A, B, C, D at i−1, i, i+1, i+2, i+3
/// (cyclic), the triangle A B C is cut off unless
///
/// - the turn at B is not to the left: (C − B) × (A − B) ≤ 1e-7; or, when n > 3,
/// - (if P is not the same point number as B) P lies on or right of A→C and not right of
///   A→B: the outline comes back into the ear at A; or
/// - (if D is not the same point number as B) D lies on or right of A→C and on or left of
///   C→B: the same at C; or
/// - the cut A–C crosses one of the outline's edges from D on round to P
///   ([`segment_intersect`]).
///
/// A refused ear moves `i` on by one. A cut removes B from the list, leaves `i` where it
/// is and sets `last` = `i`. **When `i` comes round to `last` without a cut, the ear at
/// the corner just refused is cut anyway**: a polygon that winds the other way (clockwise
/// in this plane), a self-crossing one or a degenerate one is still reduced to n − 2
/// triangles, some of them outside it or of no area.
///
/// So the triangle count is always (corners after step 2) + (holes) − 2, and the order of
/// the triangles is the order of the cuts.
///
/// The seventh argument of the original (a `short`) is not read.
pub fn triangulate_contour(points: &mut [P], indices: &mut [i32]) -> (bool, Vec<[i32; 3]>) {
    for p in points.iter_mut() {
        *p = [snap(p[0]), snap(p[1]), snap(p[2])];
    }
    // corners that repeat the corner before them
    let mut n = 0usize;
    if let Some(&end) = indices.last() {
        let mut before = end;
        for k in 0..indices.len() {
            if indices[k] != before {
                indices[n] = indices[k];
                before = indices[k];
                n += 1;
            }
        }
    }
    let mut holes = indices[..n].iter().filter(|&&i| i == -1).count() as i32;
    let mut work: Vec<i32> = indices[..n].to_vec();
    if holes != 0 && !merge_holes(points, &indices[..n], &mut holes, &mut work) {
        return (false, Vec::new());
    }
    let mut triangles = Vec::with_capacity(work.len().saturating_sub(2));
    let mut i = 0usize;
    let mut last = 0usize;
    while work.len() > 2 {
        let n = work.len();
        let (prev, next, next2, next3) = ((i + n - 1) % n, (i + 1) % n, (i + 2) % n, (i + 3) % n);
        let [p, a, b, c, d] = [prev, i, next, next2, next3].map(|k| pt(points, work[k]));
        let eps = x(EAR_EPS);
        let refused = 'ear: {
            if eps.ge(cross(b, c, a)) {
                break 'ear true;
            }
            if n == 3 {
                break 'ear false;
            }
            if work[prev] != work[next] && eps.ge(cross(a, c, p)) && cross(a, b, p).ge(x(-EAR_EPS)) {
                break 'ear true;
            }
            if work[next3] != work[next] && eps.ge(cross(a, c, d)) && eps.ge(cross(c, b, d)) {
                break 'ear true;
            }
            let mut e = d;
            let mut k = next3;
            while k != prev {
                k = (k + 1) % n;
                let f = pt(points, work[k]);
                if segment_intersect(a, c, e, f) {
                    break 'ear true;
                }
                e = f;
            }
            false
        };
        if refused {
            let step = if i + 1 == n { 0 } else { i + 1 };
            if step != last {
                i = step;
                continue;
            }
            // round the whole outline without a cut: this ear goes all the same
        }
        triangles.push([work[i], work[next], work[next2]]);
        work.remove(next);
        last = i;
        if next == 0 {
            i -= 1;
            last = i;
        }
    }
    (true, triangles)
}

/// The tolerance of the two convexity tests below: the double 0.0001 (0x086625c8,
/// 0x086625d0).
const CONVEX_EPS: f64 = 1e-4;

/// The first corner from `start` on where an outline does not turn left
/// [soft 0x0851e380 findConcavityPt]: the first i ≥ `start` with
/// (next − cur) × (previous − cur) ≤ 0.0001, or −1 when every corner up to the end of the
/// list turns left (a NaN counts as turning left). The corner before `start` is taken
/// cyclically; the search itself does not wrap. Class: model.
///
/// (An empty list makes the original divide by zero; here it answers −1.)
pub fn find_concavity_pt(points: &[P], idx: &[i32], start: i32) -> i32 {
    let n = idx.len() as i32;
    if n == 0 {
        return -1;
    }
    let at = |i: i32| pt(points, idx.get(i as usize).copied().unwrap_or(-1));
    let mut before = at((start - 1 + n) % n);
    let mut i = start;
    while i < n {
        let cur = at(i);
        let next = at((i + 1) % n);
        if cross(cur, next, before).le(x(CONVEX_EPS)) {
            return i;
        }
        before = cur;
        i += 1;
    }
    -1
}

/// Whether `count` corners of an outline, from `start` on, form a convex piece that can be
/// cut off [soft 0x0851e47c polValid]: every corner of the piece, closed from its last
/// corner back to its first, must turn left by more than 0.0001 (a NaN passes), and the
/// closing edge must cross none of the outline's edges from the piece's last corner on
/// round to its first ([`segment_intersect`]). Class: model.
pub fn pol_valid(points: &[P], idx: &[i32], start: i32, count: i32) -> bool {
    let n = idx.len() as i32;
    if n == 0 {
        return false; // the original divides by zero
    }
    let at = |i: i32| pt(points, idx.get(i as usize).copied().unwrap_or(-1));
    let end = (start + count - 1 + n) % n;
    // the corner after `i` within the piece: past its last corner comes its first
    let wrap = |i: i32| {
        let j = (i + 1) % n;
        if (start < end && end < j) || (end < start && end < j && j < start) {
            start
        } else {
            j
        }
    };
    let mut before = at(end);
    let mut i = start;
    loop {
        let cur = at(i);
        if cross(cur, at(wrap(i)), before).le(x(CONVEX_EPS)) {
            return false;
        }
        i = wrap(i);
        before = cur;
        if i == start {
            break;
        }
    }
    let (a, b) = (at(start), at(end));
    let mut k = end;
    while k != start {
        let k2 = (k + 1) % n;
        if segment_intersect(a, b, at(k), at(k2)) {
            return false;
        }
        k = k2;
    }
    true
}

/// One polygon of a mesh made ready for [`triangulate_contour`], as `SI_meshTriangulate`
/// (0x08443144) and the renderer do it before they call: the polygon's distinct vertices
/// in order of first appearance (`vertices`: their numbers in the mesh), their positions
/// sent through `SI_matrixZAlign` of the polygon's stored normal (each coordinate
/// ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ, rounded once), and per corner record the number of its
/// point, −1 for a separator.
pub struct PolygonPoints {
    pub vertices: Vec<i32>,
    pub points: Vec<P>,
    pub indices: Vec<i32>,
    pub matrix: Matrix,
}

/// See [`PolygonPoints`]. (This is the preparation inside `SI_meshTriangulate`, which is
/// not ported as a whole; the finding says what the rest of it does.)
pub fn polygon_points(mesh: &Mesh, p: usize) -> PolygonPoints {
    let poly = &mesh.polygons[p];
    let matrix = matrix_z_align(&Vector::from_array(poly.normal));
    let mut out = PolygonPoints { vertices: Vec::new(), points: Vec::new(), indices: Vec::with_capacity(poly.nodes.len()), matrix };
    for c in &poly.nodes {
        if c.vertex == -1 {
            out.indices.push(-1);
            continue;
        }
        let at = match out.vertices.iter().position(|&v| v == c.vertex) {
            Some(at) => at,
            None => {
                out.vertices.push(c.vertex);
                out.points.push(mesh.vertices.get(c.vertex as usize).map_or([0.0; 3], |v| v.pos));
                out.vertices.len() - 1
            }
        };
        out.indices.push(at as i32);
    }
    let m = &out.matrix;
    for q in &mut out.points {
        let [px, py, pz] = *q;
        let col = |j: usize| (((x(py) * m[1][j] + x(px) * m[0][j]) + x(pz) * m[2][j]) + m[3][j]).to_f32();
        *q = [col(0), col(1), col(2)];
    }
    out
}

#[cfg(test)]
mod tests;
