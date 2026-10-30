// SPDX-License-Identifier: MIT
//! Shadow rays of the add-on's light arrays (unit softspeed1, unit soft4; our own code, nothing here reproduces a
//! function of the original).
//!
//! blend2db writes a Blender light with a size as an array of n lights with ray-traced shadows (unit soft1,
//! re/tools/builder2/blend2db/src/soft.rs), and a single light with ray-traced shadows as an array of one (unit soft4),
//! and lists their names in `soft-arrays.txt` beside the database (a file the original never writes or reads). For
//! those lights only, the shadow ray is first answered in doubles (the original's walk tests every triangle listed in
//! every cell the ray crosses: about 2.3 microseconds a shadow ray on the gallery's still life, against 0.9 here), in
//! two ways:
//! - [`Bvh::shaft_hit`]: the n shadow rays of one shaded point to one array come one after the other, so the triangles
//!   near any of them (near the convex hull of the point and the array's lights, the "shaft") are found once, and each
//!   ray tests only those (4.5 on average on the still life: 76% of its array rays are answered this way);
//! - [`Bvh::any_hit`], for a shaft with more than [`MAX_CANDIDATES`] triangles (a point behind a detailed model), a ray
//!   outside its shaft, or a light that is an array of one: a bounding volume hierarchy (surface-area split) over the
//!   triangles of the frame's BSP tree, built once per frame.
//!
//! Each triangle near the ray is tested by [`hit`], the walk's own test (render.rs `exact_hit`, the port of the
//! original's `SI_Intersect`) repeated in doubles with a rigorous bound on how far the walk's extended-precision result
//! can be from it: Miss, Sure (the walk meets it, at a distance known within the bound) or Near (the walk decides).
//! [`Bvh::decide`] then follows the walk: it meets the triangles nearest first, each at least 0.0001 (trace.rs `SHADOW_PAST`) past
//! the one before, and stops at the first opaque one. The answer is used only when it is certain to be the walk's:
//! Clear (nothing met), Opaque (an opaque triangle met, the factor the umbra) or Through (only transparent triangles
//! whose material the walk does not change, at most 8: the factor computed by [`through`] with the walk's operations
//! in the walk's order and precision). Every other ray (a Near triangle on the way, gaps the bounds cannot order, a
//! textured or vertex-coloured transparent material, a light of a selective set) is traced by the original's walk
//! (`trace::s_trace`). The test `the_test_answers_as_the_walks` holds [`hit`] against the walk's test
//! (`render::probe`) on 200,000 random and borderline cases: 0 errors (a mutation with the bounds set to zero fails).
//!
//! `SOFTARRAY_CHECK=1` walks every ray too, uses the walk's factor, and counts. On the still life at 480x320 (an
//! array of 24 lights and one shadowed point light): 14.07 million rays, of which 11.86 million clear, 0.49 million
//! opaque and 1.71 million through transparent triangles agreed with the walk bit for bit, 0 disagreed, 5,912 were
//! left to the walk (0.04%). What differs from the original's run: the ray counter and the "met" marks of the
//! triangles the walk would have left (neither is read before it is set again), and a transparent triangle's material
//! adjusted by a walk that ended at an opaque one behind it (the original returns the umbra there too). Speed on the
//! still life, 480x320, one thread: 16.9 s with the walk for every ray, about 10.3 s with these answers.
//!
//! Lights of a database without the file, every Softimage scene's, are traced as before, and no hierarchy is built.

use crate::shade::ShadeState;
use crate::trace::Bsp;
use crate::types::{RVertex, Tn, Vector};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

/// The listed names and their array's number (1, 2, ...): one array a name before its last 'x' (`spot1x7` -> `spot1`).
static MEMBERS: RwLock<Option<HashMap<Vec<u8>, u32>>> = RwLock::new(None);

/// `SOFTARRAY_CHECK=1` (a check of ours): every shadow ray of an array is traced by the original's walk as well, the
/// walk's answer is the one used (the picture is the faithful one), and [`report`] counts the hierarchy's certain
/// answers that the walk agrees and disagrees with.
static CHECK: OnceLock<bool> = OnceLock::new();
/// clear agreed, opaque agreed, disagreed, unsure; ns of the hierarchy and of the walk; rays answered from a shaft's
/// candidates, rays of a shaft with too many (per ray), rays leaving their shaft (per ray); shafts built, their
/// candidates; rays through transparent triangles agreed (unit soft4), of them through more than one; rays to an
/// array of one light (unit soft4)
static COUNTS: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];

/// Under [`checking`]: why [`Bvh::decide`] left a ray unsure: too many transparent triangles, one after the opaque one
/// too close, one within the margins before the walk stops, gaps, materials, the triangle shaded.
static WHY: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

fn why(k: usize) -> Seen {
    if checking() {
        WHY[k].fetch_add(1, Ordering::Relaxed);
    }
    Seen::Unsure
}

/// Whether `SOFTARRAY_CHECK` is set to 1.
pub fn checking() -> bool {
    *CHECK.get_or_init(|| std::env::var("SOFTARRAY_CHECK").is_ok_and(|v| v == "1"))
}

/// Counts one ray under [`checking`]: the hierarchy's answer (`mine`, the factor it gives; none when unsure) against
/// the factor the walk returned, bit for bit; the nanoseconds the hierarchy and the walk took; `passes` the
/// transparent triangles of a [`Seen::Through`].
pub fn record(seen: Seen, mine: Option<[f32; 4]>, walk: [f32; 4], passes: usize, ns: (u64, u64)) {
    COUNTS[4].fetch_add(ns.0, Ordering::Relaxed);
    COUNTS[5].fetch_add(ns.1, Ordering::Relaxed);
    let same = mine.is_some_and(|m| m.iter().zip(walk.iter()).all(|(a, b)| a.to_bits() == b.to_bits()));
    let k = match seen {
        Seen::Unsure => 3,
        _ if !same => 2,
        Seen::Clear => 0,
        Seen::Opaque => 1,
        Seen::Through => {
            if passes > 1 {
                COUNTS[12].fetch_add(1, Ordering::Relaxed);
            }
            11
        }
    };
    COUNTS[k].fetch_add(1, Ordering::Relaxed);
}

/// The counts under [`checking`], for render-frames to print: none when not checking.
pub fn report() -> Option<String> {
    checking().then(|| {
        let c = COUNTS.each_ref().map(|c| c.load(Ordering::Relaxed));
        let rays = (c[0] + c[1] + c[2] + c[3] + c[11]).max(1) as f64;
        format!(
            "softarray check: clear agreed {}, opaque agreed {}, through transparent agreed {} (of them through 2 or more {}), \
             DISAGREED {}, unsure (walked) {}; ns a ray: hierarchy {:.0}, walk {:.0}; \
             shaft rays {}, per ray (many) {}, left the shaft {}, single light {}; shafts {}, candidates a shaft {:.1}; \
             unsure why: many {}, after the opaque {}, near {}, gap {}, material {}, shaded {}",
            c[0], c[1], c[11], c[12], c[2], c[3], c[4] as f64 / rays, c[5] as f64 / rays, c[6], c[7], c[8], c[13], c[9], c[10] as f64 / c[9].max(1) as f64,
            WHY[0].load(Ordering::Relaxed), WHY[1].load(Ordering::Relaxed), WHY[2].load(Ordering::Relaxed),
            WHY[3].load(Ordering::Relaxed), WHY[4].load(Ordering::Relaxed), WHY[5].load(Ordering::Relaxed)
        )
    })
}

/// The first distance a shadow ray counts, as `trace::s_trace` starts it (0.0001 as a float).
pub const SHADOW_TMIN: f32 = f32::from_bits(0x38d1_b717);

/// The name of the file blend2db writes beside a database's `db` directory: one light name a line.
pub const FILE: &str = "soft-arrays.txt";

/// Reads `<dir>/soft-arrays.txt` if there is one (render-frames: its database directory); without it nothing changes.
pub fn load(dir: &std::path::Path) {
    let names = std::fs::read(dir.join(FILE)).ok().map(|b| {
        let mut groups: HashMap<Vec<u8>, u32> = HashMap::new();
        let mut names = HashMap::new();
        for l in b.split(|&c| c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)).filter(|l| !l.is_empty()) {
            let base = l.iter().rposition(|&c| c == b'x').map_or(l, |i| &l[..i]).to_vec();
            let next = groups.len() as u32 + 1;
            let g = *groups.entry(base).or_insert(next);
            names.insert(l.to_vec(), g);
        }
        names
    });
    if let Ok(mut m) = MEMBERS.write() {
        *m = names;
    }
}

/// The number of the array (1, 2, ...) the light record named `name` is a member of, as listed by [`load`]; 0 none.
pub fn group(name: &[u8]) -> u32 {
    MEMBERS.read().ok().and_then(|m| m.as_ref().and_then(|s| s.get(name).copied())).unwrap_or(0)
}

/// What [`Bvh::any_hit`] makes of a shadow ray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Clear,
    Opaque,
    Unsure,
    /// Through transparent triangles only, all certainly met, in the order and with the materials [`Passes`] lists
    /// (unit soft4): the walk's factor is [`through`]'s.
    Through,
}

/// The most transparent triangles a [`Seen::Through`] answer lists (a ray through a glass and a ball meets 2 to 6).
pub const PASS_MAX: usize = 8;

/// The transparent triangles (their indices in the frame's triangle list) a shadow ray passes, nearest first, when the
/// answer is [`Seen::Through`]; the walk would meet each of them and only them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Passes {
    pub n: usize,
    pub tn: [usize; PASS_MAX],
}

/// One triangle of the hierarchy: its first corner and two edges (for the boxes and shafts); what the original's test
/// reads of it ([`hit`]): the stored plane (normal and distance), the normal's component along the dominant axis, the
/// two axes (a, b) of the plane the weights are measured in, and for the three weights the corner and edge they are
/// measured from, as (a, b) pairs; the triangle's index and whether it is opaque (none of the flags 0x340).
struct Tri {
    a: [f64; 3],
    e1: [f64; 3],
    e2: [f64; 3],
    n: [f64; 3],
    d: f64,
    nk: f64,
    ia: usize,
    ib: usize,
    wc: [[f64; 2]; 3],
    we: [[f64; 2]; 3],
    tn: usize,
    opaque: bool,
}

/// A node: its box; a leaf holds `count` triangles from `first`, an inner node (count 0) its two children at `first`
/// and `first + 1`.
#[derive(Clone, Copy)]
struct Node {
    lo: [f64; 3],
    hi: [f64; 3],
    first: u32,
    count: u32,
}

/// The hierarchy over a frame's ray-traced triangles.
pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<Tri>,
    /// a triangle's index (in the frame's list) to its place in `tris`, u32::MAX for one the walk never meets
    index: Vec<u32>,
    /// the largest coordinate (in size) of a corner: the scale of the original's rounding errors
    extent: f64,
    /// by array number: whether the array has one light only (blend2db lists a single light with ray-traced shadows as
    /// an array of one, unit soft4), whose rays skip the shaft
    single: Vec<bool>,
}

const LEAF: usize = 2;
const BINS: usize = 16;

impl Bvh {
    /// For a frame whose lights include an array member with ray-traced shadows: the triangles listed in the BSP tree's
    /// cells (those the original's walk can meet, without the ones flagged 0x8000, which it skips). Else none.
    pub fn for_arrays(st: &ShadeState, bsp: &Bsp) -> Option<std::sync::Arc<Bvh>> {
        if st.glb.shadows == 0 || !st.lights.iter().any(|l| l.array != 0 && l.shadow == 2) {
            return None;
        }
        let mut seen = vec![false; st.tns.len()];
        let mut list = Vec::new();
        for n in &bsp.nodes {
            if n.kind != 0 {
                continue;
            }
            let mut cur = n.list;
            while let Some(l) = cur {
                let link = bsp.links.get(l)?;
                cur = link.next;
                if link.tn < seen.len() && !seen[link.tn] {
                    seen[link.tn] = true;
                    list.push(link.tn);
                }
            }
        }
        let mut b = Bvh::build(&st.tns, &st.verts, &list)?;
        let mut count = Vec::new();
        for l in st.lights.iter().filter(|l| l.array != 0) {
            let g = l.array as usize;
            if count.len() <= g {
                count.resize(g + 1, 0u32);
            }
            count[g] += 1;
        }
        b.single = count.iter().map(|&n| n == 1).collect();
        Some(std::sync::Arc::new(b))
    }

    /// The hierarchy over the triangles `list` (a triangle without three corners: none).
    pub fn build(tns: &[Tn], verts: &[RVertex], list: &[usize]) -> Option<Bvh> {
        let sub = |p: [f64; 3], q: [f64; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
        let mut tris = Vec::with_capacity(list.len());
        for &i in list {
            let t = &tns[i];
            if t.flags & 0x8000 != 0 {
                continue;
            }
            let c = |k: usize| t.v[k].and_then(|j| verts.get(j)).map(|v| [v.world.x as f64, v.world.y as f64, v.world.z as f64]);
            let (a, b, c) = (c(0)?, c(1)?, c(2)?);
            // the original's test (render.rs `weights`): axis 1 measures in (z, y), 2 in (x, z), 3 in (y, x)
            let (ia, ib) = match t.axis {
                1 => (2, 1),
                2 => (0, 2),
                _ => (1, 0),
            };
            let nk = match t.axis {
                1 => t.n.x,
                2 => t.n.y,
                _ => t.n.z,
            } as f64;
            let v3 = |v: Vector| [v.x as f64, v.y as f64, v.z as f64];
            let ab = |p: [f64; 3]| [p[ia], p[ib]];
            let wc = [ab(b), ab(c), ab(a)];
            let we = t.edge.map(|e| ab(v3(e)));
            tris.push(Tri { a, e1: sub(b, a), e2: sub(c, a), n: v3(t.n), d: t.d as f64, nk, ia, ib, wc, we, tn: i, opaque: t.flags & 0x340 == 0 });
        }
        let bound = |t: &Tri| {
            let mut lo = t.a;
            let mut hi = t.a;
            for p in [t.e1, t.e2] {
                for k in 0..3 {
                    lo[k] = lo[k].min(t.a[k] + p[k]);
                    hi[k] = hi[k].max(t.a[k] + p[k]);
                }
            }
            (lo, hi)
        };
        let centre = |t: &Tri, k: usize| t.a[k] + (t.e1[k] + t.e2[k]) / 3.0;
        let extent = tris.iter().map(bound).fold(0.0f64, |m, (lo, hi)| lo.iter().chain(hi.iter()).fold(m, |m, v| m.max(v.abs())));
        let mut nodes = vec![Node { lo: [0.0; 3], hi: [0.0; 3], first: 0, count: tris.len() as u32 }];
        let mut todo = vec![0usize];
        while let Some(ni) = todo.pop() {
            let (first, count) = (nodes[ni].first as usize, nodes[ni].count as usize);
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            let (mut clo, mut chi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for t in &tris[first..first + count] {
                let (a, b) = bound(t);
                for k in 0..3 {
                    lo[k] = lo[k].min(a[k]);
                    hi[k] = hi[k].max(b[k]);
                    clo[k] = clo[k].min(centre(t, k));
                    chi[k] = chi[k].max(centre(t, k));
                }
            }
            nodes[ni].lo = lo;
            nodes[ni].hi = hi;
            if count <= LEAF {
                continue;
            }
            // the split of least surface-area cost over BINS bins of the centres along each axis: a large triangle (the
            // gallery's room is a 44-unit cube around everything) ends up near the root instead of widening the boxes of
            // many small ones; a median split put every big triangle in a different leaf and every ray walked to them
            let area = |lo: [f64; 3], hi: [f64; 3]| {
                let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                if d.iter().any(|v| !(*v >= 0.0)) { 0.0 } else { d[0] * d[1] + d[1] * d[2] + d[2] * d[0] }
            };
            let mut best: Option<(f64, usize, f64)> = None; // cost, axis, centre coordinate a triangle goes left below
            for k in 0..3 {
                let w = chi[k] - clo[k];
                if !(w > 0.0) {
                    continue;
                }
                let bin_of = |c: f64| (((c - clo[k]) / w * BINS as f64) as usize).min(BINS - 1);
                let mut bins = [(0usize, [f64::INFINITY; 3], [f64::NEG_INFINITY; 3]); BINS];
                for t in &tris[first..first + count] {
                    let b = &mut bins[bin_of(centre(t, k))];
                    let (a, z) = bound(t);
                    b.0 += 1;
                    for j in 0..3 {
                        b.1[j] = b.1[j].min(a[j]);
                        b.2[j] = b.2[j].max(z[j]);
                    }
                }
                // costs of the BINS - 1 planes: left sums from below, right sums from above
                let mut right = [(0usize, 0.0f64); BINS];
                let (mut n, mut blo, mut bhi) = (0usize, [f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for i in (1..BINS).rev() {
                    n += bins[i].0;
                    for j in 0..3 {
                        blo[j] = blo[j].min(bins[i].1[j]);
                        bhi[j] = bhi[j].max(bins[i].2[j]);
                    }
                    right[i] = (n, area(blo, bhi));
                }
                let (mut n, mut blo, mut bhi) = (0usize, [f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for i in 0..BINS - 1 {
                    n += bins[i].0;
                    for j in 0..3 {
                        blo[j] = blo[j].min(bins[i].1[j]);
                        bhi[j] = bhi[j].max(bins[i].2[j]);
                    }
                    let (nr, ar) = right[i + 1];
                    if n == 0 || nr == 0 {
                        continue;
                    }
                    let cost = n as f64 * area(blo, bhi) + nr as f64 * ar;
                    if best.is_none_or(|b| cost < b.0) {
                        best = Some((cost, k, clo[k] + w * (i + 1) as f64 / BINS as f64));
                    }
                }
            }
            let whole = area(lo, hi);
            let half = match best {
                // a leaf when no split pays for the extra box test (cost 1 a box, 1 a triangle) and the leaf is small
                Some((cost, _, _)) if count <= 8 && whole > 0.0 && 1.0 + cost / whole >= count as f64 => continue,
                Some((_, k, at)) => {
                    let s = &mut tris[first..first + count];
                    let mut i = 0;
                    for j in 0..s.len() {
                        if centre(&s[j], k) < at {
                            s.swap(i, j);
                            i += 1;
                        }
                    }
                    if i == 0 || i == count {
                        count / 2
                    } else {
                        i
                    }
                }
                // every centre in one point: halves
                None => count / 2,
            };
            let child = nodes.len();
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: first as u32, count: half as u32 });
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], first: (first + half) as u32, count: (count - half) as u32 });
            nodes[ni].first = child as u32;
            nodes[ni].count = 0;
            todo.push(child);
            todo.push(child + 1);
        }
        let mut index = vec![u32::MAX; tns.len()];
        for (i, t) in tris.iter().enumerate() {
            index[t.tn] = i as u32;
        }
        Some(Bvh { nodes, tris, index, extent, single: Vec::new() })
    }

    /// The margin, a length: 2e-6 of the largest coordinate of the scene or the ray's origin (plus 2e-6). The point the
    /// original's test takes for a hit lies on its stored plane and inside its stored edges, both from floats (6e-8
    /// of their size), so well within it of the triangle: the boxes and shafts are widened by it.
    fn margin(&self, o: [f64; 3]) -> f64 {
        2e-6 * (1.0 + self.extent.max(size(o)))
    }

    /// The shadow ray from `o` along `d` (as `s_trace` gets it: `d` the unit direction to the light, `tmax` its
    /// distance, `tmin` 0.0001, `from` the triangle shaded). A [`Seen::Through`] answer lists its triangles in `pass`.
    #[allow(clippy::too_many_arguments)]
    pub fn any_hit(&self, st: &ShadeState, o: Vector, d: Vector, from: usize, tmin: f32, tmax: f32, pass: &mut Passes) -> Seen {
        pass.n = 0;
        let o = [o.x as f64, o.y as f64, o.z as f64];
        let d = [d.x as f64, d.y as f64, d.z as f64];
        let (tmin, tmax) = (tmin as f64, tmax as f64);
        if !(o.iter().chain(d.iter()).all(|v| v.is_finite()) && tmax.is_finite() && tmax > tmin) || self.tris.is_empty() {
            return if self.tris.is_empty() && tmax.is_finite() { Seen::Clear } else { Seen::Unsure };
        }
        let e = self.margin(o);
        let inv = d.map(|c| if c.abs() > 1e-12 { 1.0 / c } else { 0.0 });
        let mut g = Gather::new();
        // the part of the segment a box must meet: [tmin - e, the horizon (or tmax) + e]
        let enter = |n: &Node, t_end: f64| -> Option<f64> {
            let (mut t0, mut t1) = (tmin - e, t_end + e);
            for k in 0..3 {
                let (lo, hi) = (n.lo[k] - e, n.hi[k] + e);
                if inv[k] == 0.0 {
                    if o[k] < lo || o[k] > hi {
                        return None;
                    }
                } else {
                    let (a, b) = ((lo - o[k]) * inv[k], (hi - o[k]) * inv[k]);
                    t0 = t0.max(a.min(b));
                    t1 = t1.min(a.max(b));
                }
            }
            (t0 <= t1).then_some(t0)
        };
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            let t_end = g.horizon().min(tmax);
            if enter(n, t_end).is_none() {
                continue;
            }
            if n.count > 0 {
                for i in n.first..n.first + n.count {
                    let t = &self.tris[i as usize];
                    if t.tn == from {
                        continue;
                    }
                    g.add(hit(o, d, t, tmin, tmax), t, i);
                }
                continue;
            }
            if sp + 2 > stack.len() {
                return Seen::Unsure;
            }
            // the nearer child is looked at first
            let (c0, c1) = (n.first, n.first + 1);
            let t_end = g.horizon().min(tmax);
            let (a, b) = (enter(&self.nodes[c0 as usize], t_end), enter(&self.nodes[c1 as usize], t_end));
            match (a, b) {
                (Some(a), Some(b)) => {
                    let (near, far) = if a <= b { (c0, c1) } else { (c1, c0) };
                    stack[sp] = far;
                    stack[sp + 1] = near;
                    sp += 2;
                }
                (Some(_), None) => {
                    stack[sp] = c0;
                    sp += 1;
                }
                (None, Some(_)) => {
                    stack[sp] = c1;
                    sp += 1;
                }
                (None, None) => {}
            }
        }
        self.decide(st, &mut g, o, d, from, tmax, pass)
    }

    /// The answer from what the triangles near a ray gave (unit soft4; before, any transparent triangle met made the
    /// ray unsure). The walk meets the triangles along the ray nearest first, each at least 0.0001 (trace.rs `SHADOW_PAST`) past the
    /// one before (`tmin` becomes the hit's distance plus 0.0001), and stops at the first opaque one. The answer is
    /// certain when every triangle it would meet up to there is certainly met, in an order and with gaps that the
    /// margins cannot change, no triangle within the margins could be met before it stops, and what the walk does at
    /// a transparent triangle changes nothing but the factor ([`plain`]): then Opaque (an opaque one is met), Through
    /// (transparent ones only, listed in `pass`) or Clear.
    #[allow(clippy::too_many_arguments)]
    fn decide(&self, st: &ShadeState, g: &mut Gather, o: [f64; 3], d: [f64; 3], from: usize, tmax: f64, pass: &mut Passes) -> Seen {
        pass.n = 0;
        if g.overflow {
            return why(0);
        }
        let tr = &mut g.tr[..g.n];
        for i in 1..tr.len() {
            let mut j = i;
            while j > 0 && tr[j - 1].0 > tr[j].0 {
                tr.swap(j - 1, j);
                j -= 1;
            }
        }
        let (op, et_op) = g.op;
        // the transparent triangles before the opaque one (all of them without one)
        let k = tr.iter().take_while(|h| h.0 < op).count();
        if op.is_finite() {
            // one met after the opaque one must be certainly after it
            if let Some(&(h, et, _)) = tr.get(k) {
                if !(h - et > op + et_op + order(h)) {
                    return why(1);
                }
            }
        }
        // a triangle within the margins that the walk may meet before it stops (without an opaque one: anywhere)
        let end = if op.is_finite() { op + et_op + order(op) } else { f64::INFINITY };
        if g.near.is_finite() && !(g.near > end) {
            return why(2);
        }
        if k > PASS_MAX {
            return why(0);
        }
        // nearest first, each certainly SHADOW_PAST past the one before
        let mut prev: Option<(f64, f64)> = None;
        for (h, et) in tr[..k].iter().map(|h| (h.0, h.1)).chain(op.is_finite().then_some((op, et_op))) {
            if let Some((ph, pet)) = prev {
                if !(h - et > ph + pet + SHADOW_PAST + order(h)) {
                    return why(3);
                }
            }
            prev = Some((h, et));
        }
        if k > 0 {
            for &(_, _, i) in tr[..k].iter() {
                if !plain(st, self.tris[i as usize].tn) {
                    return why(4);
                }
            }
            // after the first one the walk no longer leaves out the triangle shaded: it must lie certainly before (the
            // walk's next tmin is float(float(t) + 0.0001))
            let (h1, et1) = (tr[0].0, tr[0].1);
            if let Some(&j) = self.index.get(from) {
                if j != u32::MAX && !matches!(hit(o, d, &self.tris[j as usize], h1 - et1 - order(h1) + SHADOW_PAST, tmax), Hit::Miss) {
                    return why(5);
                }
            }
        }
        if op.is_finite() {
            return Seen::Opaque;
        }
        if k == 0 {
            return Seen::Clear;
        }
        pass.n = k;
        for (s, h) in pass.tn.iter_mut().zip(tr[..k].iter()) {
            *s = self.tris[h.2 as usize].tn;
        }
        Seen::Through
    }

    /// The shadow ray of light `light` (a member of array `st.lights[light].array`) from `o` along `d`, as
    /// [`Bvh::any_hit`] takes it, answered from `sh`: the shadow rays of one shaded point to the lights of one array
    /// come one after the other with the same origin and triangle, so the triangles near any of them (the "shaft": the
    /// convex hull of the point and the array's lights) are looked up once, and each ray then tests only those. A
    /// shaft with more than [`MAX_CANDIDATES`] (a point behind a detailed model), a point whose lights are spread over
    /// more than 60 degrees, or a ray that leaves its shaft, is answered by [`Bvh::any_hit`].
    #[allow(clippy::too_many_arguments)]
    pub fn shaft_hit(&self, sh: &mut Shaft, st: &ShadeState, light: usize, o: Vector, d: Vector, from: usize, tmin: f32, tmax: f32, pass: &mut Passes) -> Seen {
        pass.n = 0;
        let group = st.lights[light].array;
        let check = checking();
        if self.single.get(group as usize).copied().unwrap_or(false) {
            // one light: the shaft is its ray
            if check {
                COUNTS[13].fetch_add(1, Ordering::Relaxed);
            }
            return self.any_hit(st, o, d, from, tmin, tmax, pass);
        }
        let key = ([o.x.to_bits(), o.y.to_bits(), o.z.to_bits()], from, group);
        if sh.key != Some(key) {
            sh.key = Some(key);
            self.build_shaft(sh, st, group, [o.x as f64, o.y as f64, o.z as f64], from);
            if check {
                COUNTS[9].fetch_add(1, Ordering::Relaxed);
                COUNTS[10].fetch_add(sh.cand.len() as u64, Ordering::Relaxed);
            }
        }
        let o64 = [o.x as f64, o.y as f64, o.z as f64];
        let d64 = [d.x as f64, d.y as f64, d.z as f64];
        let (lo, hi) = (tmin as f64, tmax as f64);
        if sh.many || !(o64.iter().chain(d64.iter()).all(|v| v.is_finite()) && hi.is_finite() && hi > lo) || self.tris.is_empty() {
            if check {
                COUNTS[7].fetch_add(1, Ordering::Relaxed);
            }
            return self.any_hit(st, o, d, from, tmin, tmax, pass);
        }
        let e = self.margin(o64);
        // the segment the hierarchy looks at, [tmin - e, tmax + e], must lie inside the shaft (widened by m / 2): its
        // ends are looked at, the shaft is convex
        let ends = [d64.map(|c| c * (lo - e)), d64.map(|c| c * (hi + e))];
        if sh.planes.iter().any(|&(n, b)| ends.iter().any(|w| dot(*w, n) > b - 0.5 * sh.m)) {
            if check {
                COUNTS[8].fetch_add(1, Ordering::Relaxed);
            }
            return self.any_hit(st, o, d, from, tmin, tmax, pass);
        }
        if check {
            COUNTS[6].fetch_add(1, Ordering::Relaxed);
        }
        let mut g = Gather::new();
        for &i in &sh.cand {
            let t = &self.tris[i as usize];
            g.add(hit(o64, d64, t, lo, hi), t, i);
        }
        self.decide(st, &mut g, o64, d64, from, hi, pass)
    }

    /// Fills `sh` for the point `p` (triangle `from` left out) and the lights of array `group`: planes (x - p) . n <= b
    /// that every segment from p to one of the lights keeps to, widened by m, and the triangles not wholly beyond one
    /// of them. Every bound is the largest value over the hull's corners (p and the lights), so the planes hold for any
    /// choice of normals; the normals are chosen to fit the hull: 8 sides around the mean direction a to the lights,
    /// the caps along a, and the 6 axes.
    fn build_shaft(&self, sh: &mut Shaft, st: &ShadeState, group: u32, p: [f64; 3], from: usize) {
        sh.planes.clear();
        sh.cand.clear();
        sh.ws.clear();
        sh.many = true;
        // m: 4 margins (the original's hit points lie well within one) and 1e-3, so that the ray's first 0.0001 is inside
        sh.m = 4.0 * self.margin(p) + 1e-3;
        for l in st.lights.iter().filter(|l| l.array == group) {
            // as `shade::lights` aims the ray: kinds 3 and 4 along their direction to FAR_LIGHT, the others at the position
            let w = if (l.kind.wrapping_sub(3) as u16) <= 1 {
                [l.dir.x as f64 * FAR_LIGHT, l.dir.y as f64 * FAR_LIGHT, l.dir.z as f64 * FAR_LIGHT]
            } else {
                [l.pos.x as f64 - p[0], l.pos.y as f64 - p[1], l.pos.z as f64 - p[2]]
            };
            sh.ws.push(w);
        }
        let len = |w: [f64; 3]| dot(w, w).sqrt();
        let mut a = [0.0; 3];
        for &w in &sh.ws {
            let l = len(w);
            if !(l > 0.0 && l.is_finite()) {
                return;
            }
            for k in 0..3 {
                a[k] += w[k] / l;
            }
        }
        let al = len(a);
        if sh.ws.is_empty() || !(al > 0.0) {
            return;
        }
        let a = a.map(|c| c / al);
        if sh.ws.iter().any(|&w| !(dot(w, a) > 0.5 * len(w))) {
            return;
        }
        let (e1, e2) = basis(a);
        for k in 0..8 {
            let (s, c) = (k as f64 * std::f64::consts::FRAC_PI_4).sin_cos();
            let u = [e1[0] * c + e2[0] * s, e1[1] * c + e2[1] * s, e1[2] * c + e2[2] * s];
            let slope = sh.ws.iter().map(|&w| dot(w, u) / dot(w, a)).fold(f64::NEG_INFINITY, f64::max);
            let n = [u[0] - slope * a[0], u[1] - slope * a[1], u[2] - slope * a[2]];
            let nl = len(n);
            sh.planes.push((n.map(|c| c / nl), 0.0));
        }
        sh.planes.push((a.map(|c| -c), 0.0));
        sh.planes.push((a, 0.0));
        for k in 0..3 {
            let mut n = [0.0; 3];
            n[k] = 1.0;
            sh.planes.push((n, 0.0));
            n[k] = -1.0;
            sh.planes.push((n, 0.0));
        }
        for pl in sh.planes.iter_mut() {
            pl.1 = sh.ws.iter().map(|&w| dot(w, pl.0)).fold(0.0f64, f64::max) + sh.m;
        }
        // the candidates: nodes whose box is not wholly beyond a plane, then triangles not wholly beyond one
        let beyond_box = |n: &Node| {
            sh.planes.iter().any(|&(q, b)| (0..3).map(|k| q[k] * (if q[k] > 0.0 { n.lo[k] } else { n.hi[k] } - p[k])).sum::<f64>() > b)
        };
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let n = &self.nodes[ni as usize];
            if beyond_box(n) {
                continue;
            }
            if n.count == 0 {
                stack.push(n.first);
                stack.push(n.first + 1);
                continue;
            }
            for i in n.first..n.first + n.count {
                let t = &self.tris[i as usize];
                if t.tn == from {
                    continue;
                }
                let c = [t.a, [t.a[0] + t.e1[0], t.a[1] + t.e1[1], t.a[2] + t.e1[2]], [t.a[0] + t.e2[0], t.a[1] + t.e2[1], t.a[2] + t.e2[2]]];
                let rel = c.map(|c| [c[0] - p[0], c[1] - p[1], c[2] - p[2]]);
                if sh.planes.iter().any(|&(q, b)| rel.iter().all(|r| dot(*r, q) > b)) {
                    continue;
                }
                if sh.cand.len() == MAX_CANDIDATES {
                    sh.cand.clear();
                    return;
                }
                sh.cand.push(i);
            }
        }
        sh.many = false;
    }
}

/// A shaft with more candidate triangles than this is not used: each ray is answered by [`Bvh::any_hit`].
pub const MAX_CANDIDATES: usize = 32;

/// `shade::lights`' distance to a light of kind 3 or 4 (100000 as a float).
const FAR_LIGHT: f64 = 100000.0;

/// The shaft of the last shaded point and array (one per tracer: [`Bvh::shaft_hit`]).
#[derive(Default)]
pub struct Shaft {
    /// the origin's bits, the triangle shaded, the array
    key: Option<([u32; 3], usize, u32)>,
    /// unit normal n and bound b: (x - p) . n <= b over the shaft, widened by `m`
    planes: Vec<([f64; 3], f64)>,
    /// indices of the triangles (in the hierarchy's order) near the shaft
    cand: Vec<u32>,
    /// the lights, from p
    ws: Vec<[f64; 3]>,
    /// no candidates: every ray of this shaft is answered by [`Bvh::any_hit`]
    many: bool,
    m: f64,
}

/// Two unit vectors at right angles to the unit vector `a` and to each other.
fn basis(a: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let t = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let e1 = cross(a, t);
    let l = dot(e1, e1).sqrt();
    let e1 = e1.map(|c| c / l);
    (e1, cross(a, e1))
}

fn cross(p: [f64; 3], q: [f64; 3]) -> [f64; 3] {
    [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]]
}

fn dot(p: [f64; 3], q: [f64; 3]) -> f64 {
    p[0] * q[0] + p[1] * q[1] + p[2] * q[2]
}

/// The walk's step past a transparent triangle it met (trace.rs: `tmin` becomes the hit's distance plus 0.0001).
const SHADOW_PAST: f64 = 1e-4;

/// The largest coordinate of `p` in size.
fn size(p: [f64; 3]) -> f64 {
    p.iter().fold(0.0f64, |m, v| m.max(v.abs()))
}

/// Beyond the nearest opaque triangle (or the first place a triangle within the margins may be met), by this much,
/// nothing found changes the answer: [`Gather::horizon`].
const PRUNE: f64 = 1e-3;

/// The most transparent triangles [`Gather`] keeps for one ray; more: unsure.
const TR_MAX: usize = 16;

/// Slack for the rounding of a distance to a float (the walk keeps `t` and `tmin` as floats).
fn order(h: f64) -> f64 {
    1e-6 * (1.0 + h.abs())
}

/// What the triangles tested against one shadow ray gave ([`hit`]).
struct Gather {
    /// the nearest opaque triangle certainly met: distance and its margin (infinite: none)
    op: (f64, f64),
    /// the least distance from which a triangle within the margins may be met (infinite: none)
    near: f64,
    /// the transparent triangles certainly met: distance, margin, index in [`Bvh::tris`]
    tr: [(f64, f64, u32); TR_MAX],
    n: usize,
    overflow: bool,
}

impl Gather {
    fn new() -> Gather {
        Gather { op: (f64::INFINITY, 0.0), near: f64::INFINITY, tr: [(0.0, 0.0, 0); TR_MAX], n: 0, overflow: false }
    }

    #[inline(always)]
    fn add(&mut self, r: Hit, t: &Tri, i: u32) {
        match r {
            Hit::Miss => {}
            Hit::Sure(h, et) if t.opaque => {
                if h < self.op.0 {
                    self.op = (h, et);
                }
            }
            Hit::Sure(h, et) => {
                if self.n < TR_MAX {
                    self.tr[self.n] = (h, et, i);
                    self.n += 1;
                } else {
                    self.overflow = true;
                }
            }
            Hit::Near(lo) => self.near = self.near.min(lo),
        }
    }

    /// The distance beyond which no triangle changes the answer: [`PRUNE`] past the nearest opaque triangle's margin or
    /// the first place a triangle within the margins may be met.
    fn horizon(&self) -> f64 {
        (self.op.0 + self.op.1).min(self.near) + PRUNE
    }
}

/// Whether the walk, meeting the transparent triangle `tn`, changes nothing but its factor: no vertex colours (which
/// set the material's working colours from the hit point), no texture stage (flag 0x400: `shadow_map`), no static
/// blur (a factor from the hit's normal). The factor then reads only the material's working opacity and diffuse
/// colour, as they are when the ray is traced ([`through`]).
fn plain(st: &ShadeState, tn: usize) -> bool {
    st.tns.get(tn).is_some_and(|t| {
        t.flags & 0x400 == 0 && st.materials.get(t.material as usize).is_some_and(|m| m.vcol_rgb == 1 && m.vcol_alpha == 1 && m.static_blur == 0)
    })
}

/// The factor the walk returns for a [`Seen::Through`] ray of light `light` (unit soft4): for each transparent
/// triangle of `pass`, nearest first, its material's share as `trace::s_trace_walk` computes it (flags 0x300: one
/// minus the working opacity, below a half twice that times the diffuse colour, above it a blend towards white; the
/// static blur factor is 1 for [`plain`] materials), then the umbra blend of a ray that passed something. The same
/// operations in the same order and precision as the walk's, so the same bits (`SOFTARRAY_CHECK=1` compares them).
pub fn through(st: &ShadeState, light: usize, pass: &Passes) -> [f32; 4] {
    use si_x87::{fmov, x, F80};
    const SHADOW_HALF: f64 = 0.5;
    let umbra = fmov(st.lights[light].umbra);
    let blur = 1.0f32;
    let mut left = [1.0f32; 3];
    for &hit in &pass.tn[..pass.n] {
        let flags = st.tns[hit].flags;
        let m = &st.materials[st.tns[hit].material as usize];
        if flags & 0x300 != 0 || m.static_blur != 0 {
            let k = (F80::ONE - x(blur) * m.work.opacity).to_f32();
            let k = if x(k).lt(x(0.0f32)) { x(0.0f32) } else { x(k) };
            if x(SHADOW_HALF).gt(k) {
                let k2 = (k + k).to_f32();
                for i in 0..3 {
                    left[i] = ((x(k2) * m.work.diffuse[i]) * left[i]).to_f32();
                }
            } else {
                let w = ((k - SHADOW_HALF) + (k - SHADOW_HALF)).to_f32();
                let rest = F80::ONE - x(w);
                for i in 0..3 {
                    left[i] = ((rest * m.work.diffuse[i] + w) * left[i]).to_f32();
                }
            }
        }
    }
    if pass.n > 0 {
        let a = (F80::ONE - x(umbra)).to_f32();
        for f in &mut left {
            *f = (x(a) * *f + umbra).to_f32();
        }
    }
    [left[0], left[1], left[2], st.glb.unset]
}

enum Hit {
    Miss,
    /// certainly a hit, at this distance (the walk's within the bound given), its float below the light's distance
    Sure(f64, f64),
    /// the walk may or may not meet it (a weight, the distance or its float too close to a limit to tell, or nearly
    /// parallel): the original's walk decides; the least distance from which it may meet it
    Near(f64),
}

/// 2^-53, the unit roundoff of a double.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// The original's |direction . normal| below which a triangle is parallel to the ray (render.rs).
const PARALLEL: f64 = 1e-8;

/// What the walk's test of triangle `t` (render.rs `exact_hit`, the port of the original's `SI_Intersect`) gives on
/// the ray from `o` along `d` (floats) with `tmin` (as the walk has it) and the light's distance `tmax`: the same
/// formula on the same stored floats (the triangle's plane normal and distance, its edges and corners), computed in
/// doubles with a bound on how far the walk's extended result can be from it, as render.rs `certain_miss` bounds it
/// (the products of two floats are exact in a double; each sum's result is within its roundings times the sum of its
/// terms' sizes, in a double as in an extended; the bounds take twice that and more). A hit needs |d . n| not below
/// 1e-8, the distance t not below tmin, the point o + float(t) d inside the three weights, and (the walk's test
/// against the light) float(t) below `tmax`. Sure or Miss only where the bounds decide every step.
#[inline(always)]
fn hit(o: [f64; 3], d: [f64; 3], t: &Tri, tmin: f64, tmax: f64) -> Hit {
    let n = t.n;
    // direction . normal: two roundings
    let (a0, a1, a2) = (d[2] * n[2], d[1] * n[1], n[0] * d[0]);
    let dn = a0 + (a1 + a2);
    let edn = 8.0 * U * (a0.abs() + a1.abs() + a2.abs());
    let adn = dn.abs();
    if adn + edn < PARALLEL * (1.0 - 1e-9) {
        return Hit::Miss;
    }
    if !(adn - edn > PARALLEL * (1.0 + 1e-9)) {
        return Hit::Near(tmin);
    }
    // t = (D - origin . normal) / (direction . normal): three roundings, then the quotient
    let (b0, b1, b2) = (o[2] * n[2], o[1] * n[1], o[0] * n[0]);
    let num = t.d - (b0 + (b1 + b2));
    let enm = 8.0 * U * (t.d.abs() + b0.abs() + b1.abs() + b2.abs());
    let h = num / dn;
    let et = 2.0 * (enm + h.abs() * edn) / (adn - edn) + 8.0 * U * h.abs();
    let (lo, hi) = (h - et, h + et);
    if !(lo.is_finite() && hi.is_finite()) {
        return Hit::Near(tmin);
    }
    // below tmin, or its float beyond the light's distance (rounding to float is monotonic)
    if hi < tmin || lo as f32 as f64 > tmax {
        return Hit::Miss;
    }
    let near = Hit::Near(lo.max(tmin));
    // the point is computed from float(t): only when the whole interval rounds to one float
    let tf = lo as f32;
    if tf != hi as f32 {
        return near;
    }
    let tf = tf as f64;
    let (pa, pb) = (o[t.ia] + tf * d[t.ia], o[t.ib] + tf * d[t.ib]);
    let (wlo, whi) = if t.nk > 0.0 { (0.0, t.nk) } else { (t.nk, 0.0) };
    let mut inside = true;
    for k in 0..3 {
        let (c, e) = (t.wc[k], t.we[k]);
        let w = (pa - c[0]) * e[1] - (pb - c[1]) * e[0];
        let ew = 16.0 * U * ((pa.abs() + c[0].abs()) * e[1].abs() + (pb.abs() + c[1].abs()) * e[0].abs());
        if w + ew < wlo || w - ew > whi {
            return Hit::Miss;
        }
        inside &= w - ew >= wlo && w + ew <= whi;
    }
    if inside && lo >= tmin && tf < tmax {
        Hit::Sure(h, et)
    } else {
        near
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(tris: &[([[f32; 3]; 3], u16)]) -> Bvh {
        let (tns, verts) = parts(tris);
        Bvh::build(&tns, &verts, &(0..tns.len()).collect::<Vec<_>>()).unwrap()
    }

    fn parts(tris: &[([[f32; 3]; 3], u16)]) -> (Vec<Tn>, Vec<RVertex>) {
        let (mut tns, mut verts) = (Vec::new(), Vec::new());
        for (p, flags) in tris {
            let base = verts.len();
            for c in p {
                verts.push(RVertex { world: Vector::new(c[0], c[1], c[2]), ..RVertex::default() });
            }
            tns.push(Tn { v: [Some(base), Some(base + 1), Some(base + 2)], flags: *flags, ..planes(p) });
        }
        (tns, verts)
    }

    /// A triangle's stored plane, axis and edges, in floats (normal (v1 - v0) x (v2 - v0), distance normal . v0).
    fn planes(p: &[[f32; 3]; 3]) -> Tn {
        let v = p.map(|c| Vector::new(c[0], c[1], c[2]));
        let sub = |a: Vector, b: Vector| Vector::new(a.x - b.x, a.y - b.y, a.z - b.z);
        let (a, b) = (sub(v[1], v[0]), sub(v[2], v[0]));
        let n = Vector::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
        let d = n.x * v[0].x + n.y * v[0].y + n.z * v[0].z;
        let axis = if n.x.abs() >= n.y.abs() && n.x.abs() >= n.z.abs() { 1 } else if n.y.abs() >= n.z.abs() { 2 } else { 3 };
        Tn { n, d, axis, edge: [sub(v[2], v[1]), sub(v[0], v[2]), sub(v[1], v[0])], ..Tn::default() }
    }

    /// A state whose triangles are `tris`, each with material 0 (plain: no vertex colours, no blur) or, for flag
    /// 0x1000 (ours, for the tests), material 1 with vertex colours; materials of opacity 0.3 and 0.8.
    fn state(tris: &[([[f32; 3]; 3], u16)]) -> ShadeState {
        let (mut tns, verts) = parts(tris);
        for t in tns.iter_mut() {
            if t.flags & 0x1000 != 0 {
                t.material = 1;
            }
        }
        let mut st = ShadeState { tns: tns.into(), verts: verts.into(), ..ShadeState::default() };
        let mut m = crate::shade::ShadeMaterial { vcol_rgb: 1, vcol_alpha: 1, ..Default::default() };
        m.work.opacity = 0.3;
        m.work.diffuse = [0.9, 0.5, 0.2, 1.0];
        st.materials.push(m.clone());
        m.vcol_rgb = 0;
        st.materials.push(m);
        st.lights.push(crate::shade::ShadeLight { umbra: 0.25, ..Default::default() });
        st
    }

    const UP: Vector = Vector { x: 0.0, y: 0.0, z: 1.0 };

    fn tri_at(x: f32, z: f32, flags: u16) -> ([[f32; 3]; 3], u16) {
        ([[x - 1.0, -1.0, z], [x + 1.0, -1.0, z], [x, 1.0, z]], flags)
    }

    #[test]
    fn clear_opaque_and_unsure() {
        let b = scene(&[tri_at(-3.0, 2.0, 0), tri_at(3.0, 2.0, 0x100)]);
        // no materials: a transparent triangle is never plain
        let st0 = ShadeState::default();
        let mut ps = Passes::default();
        let tmin = SHADOW_TMIN;
        let at = |x: f32, y: f32| Vector::new(x, y, 0.0);
        // nothing above x = 10
        assert_eq!(b.any_hit(&st0, at(10.0, 0.0), UP, usize::MAX, tmin, 5.0, &mut ps), Seen::Clear);
        // the opaque triangle above x = -3, the transparent one above x = 3
        assert_eq!(b.any_hit(&st0, at(-3.0, 0.0), UP, usize::MAX, tmin, 5.0, &mut ps), Seen::Opaque);
        assert_eq!(b.any_hit(&st0, at(3.0, 0.0), UP, usize::MAX, tmin, 5.0, &mut ps), Seen::Unsure);
        // the light below the triangle: clear; the triangle shaded is not looked at
        assert_eq!(b.any_hit(&st0, at(-3.0, 0.0), UP, usize::MAX, tmin, 1.5, &mut ps), Seen::Clear);
        assert_eq!(b.any_hit(&st0, at(-3.0, 0.0), UP, 0, tmin, 5.0, &mut ps), Seen::Clear);
        // through a corner, or ending at the triangle: the original's walk decides
        assert_eq!(b.any_hit(&st0, at(-4.0, -1.0), UP, usize::MAX, tmin, 5.0, &mut ps), Seen::Unsure);
        assert_eq!(b.any_hit(&st0, at(-3.0, 0.0), UP, usize::MAX, tmin, 2.0, &mut ps), Seen::Unsure);
    }

    #[test]
    fn the_nearest_decides() {
        let d = {
            let (dx, dz) = (8.0f32, 2.0f32);
            let l = (dx * dx + dz * dz).sqrt();
            Vector::new(dx / l, 0.0, dz / l)
        };
        let o = Vector::new(-5.0, 0.0, 0.0);
        let st0 = ShadeState::default();
        let mut ps = Passes::default();
        // the ray from x = -5 meets z = 1 at x = -1 and z = 2 at x = 3
        let b = scene(&[tri_at(3.0, 2.0, 0)]);
        assert_eq!(b.any_hit(&st0, o, d, usize::MAX, SHADOW_TMIN, 20.0, &mut ps), Seen::Opaque);
        // a transparent triangle (not plain here) in front of the opaque one: unsure; behind it: opaque
        let b = scene(&[tri_at(3.0, 2.0, 0), tri_at(-1.0, 1.0, 0x200)]);
        assert_eq!(b.any_hit(&st0, o, d, usize::MAX, SHADOW_TMIN, 20.0, &mut ps), Seen::Unsure);
        let b = scene(&[tri_at(3.0, 2.0, 0), tri_at(7.0, 3.0, 0x200)]);
        assert_eq!(b.any_hit(&st0, o, d, usize::MAX, SHADOW_TMIN, 20.0, &mut ps), Seen::Opaque);
    }

    #[test]
    fn through_transparent_triangles() {
        let at = |x: f32| Vector::new(x, 0.0, 0.0);
        let mut ps = Passes::default();
        let run = |tris: &[([[f32; 3]; 3], u16)], o: Vector, tmax: f32, ps: &mut Passes| {
            let st = state(tris);
            let b = Bvh::build(&st.tns, &st.verts, &(0..st.tns.len()).collect::<Vec<_>>()).unwrap();
            b.any_hit(&st, o, UP, usize::MAX, SHADOW_TMIN, tmax, ps)
        };
        // two plain transparent triangles: through both, nearest first
        let two = [tri_at(0.0, 3.0, 0x100), tri_at(0.0, 1.0, 0x200)];
        assert_eq!(run(&two, at(0.0), 5.0, &mut ps), Seen::Through);
        assert_eq!(&ps.tn[..ps.n], &[1, 0]);
        // the light between them: through the lower one only
        assert_eq!(run(&two, at(0.0), 2.0, &mut ps), Seen::Through);
        assert_eq!(&ps.tn[..ps.n], &[1]);
        // an opaque triangle behind them: opaque; in front of them: opaque
        assert_eq!(run(&[two[0], two[1], tri_at(0.0, 4.0, 0)], at(0.0), 5.0, &mut ps), Seen::Opaque);
        assert_eq!(run(&[two[0], two[1], tri_at(0.0, 0.5, 0)], at(0.0), 5.0, &mut ps), Seen::Opaque);
        // an opaque one closer behind a transparent one than the walk's step past it: the walk skips it
        assert_eq!(run(&[tri_at(0.0, 1.0, 0x100), tri_at(0.0, 1.00005, 0)], at(0.0), 5.0, &mut ps), Seen::Unsure);
        // two transparent ones as close: unsure
        assert_eq!(run(&[tri_at(0.0, 1.0, 0x100), tri_at(0.0, 1.00005, 0x100)], at(0.0), 5.0, &mut ps), Seen::Unsure);
        // vertex colours (material 1): unsure
        assert_eq!(run(&[tri_at(0.0, 1.0, 0x1100)], at(0.0), 5.0, &mut ps), Seen::Unsure);
        // the texture stage (0x400): unsure
        assert_eq!(run(&[tri_at(0.0, 1.0, 0x500)], at(0.0), 5.0, &mut ps), Seen::Unsure);
        // the factor: as the walk computes it (k = 0.7 > 0.5: w = 0.4, (0.6 d + 0.4) twice, then the umbra blend)
        let st = state(&two);
        let p = Passes { n: 2, tn: [1, 0, 0, 0, 0, 0, 0, 0] };
        let f = through(&st, 0, &p);
        for (i, d) in [0.9f64, 0.5, 0.2].iter().enumerate() {
            let one = 0.6 * d + 0.4;
            let want = 0.75 * one * one + 0.25;
            assert!((f[i] as f64 - want).abs() < 1e-6, "{i} {} {want}", f[i]);
        }
    }

    #[test]
    fn the_triangle_shaded_is_met_again_only_after_the_first() {
        // the point shaded lies on a slanted transparent triangle (index 0); the ray passes a plain one above it
        let tris = [([[-1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [0.0, 1.0, 0.0]], 0x100u16), tri_at(0.0, 2.0, 0x100)];
        let st = state(&tris);
        let b = Bvh::build(&st.tns, &st.verts, &[0, 1]).unwrap();
        let mut ps = Passes::default();
        assert_eq!(b.any_hit(&st, Vector::new(0.0, 0.0, 0.0), UP, 0, SHADOW_TMIN, 5.0, &mut ps), Seen::Through);
        assert_eq!(&ps.tn[..ps.n], &[1]);
    }

    #[test]
    fn many_triangles_agree_with_a_plain_test() {
        // a grid of small triangles over [-4, 4]^2 at heights 1..3, every third transparent (plain); rays up from
        // below, slanted: the hierarchy's answer against testing every triangle
        let mut tris = Vec::new();
        for i in 0..40 {
            for j in 0..40 {
                let (x, y) = (-4.0 + 0.2 * i as f32, -4.0 + 0.2 * j as f32);
                let z = 1.0 + ((i * 7 + j * 3) % 20) as f32 * 0.1;
                if (i + j) % 2 == 0 {
                    tris.push(([[x, y, z], [x + 0.15, y, z], [x, y + 0.15, z]], if (i * j) % 3 == 0 { 0x100 } else { 0 }));
                }
            }
        }
        let st = state(&tris);
        let b = Bvh::build(&st.tns, &st.verts, &(0..st.tns.len()).collect::<Vec<_>>()).unwrap();
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        let (mut seen, mut sure) = ([0usize; 4], 0);
        let (mut ps, mut pw) = (Passes::default(), Passes::default());
        for _ in 0..4000 {
            let o = Vector::new(-4.0 + 8.0 * rnd(), -4.0 + 8.0 * rnd(), 0.0);
            let (dx, dy) = (rnd() - 0.5, rnd() - 0.5);
            let l = (dx * dx + dy * dy + 1.0).sqrt();
            let d = Vector::new(dx / l, dy / l, 1.0 / l);
            let got = b.any_hit(&st, o, d, usize::MAX, SHADOW_TMIN, 5.0, &mut ps);
            // the plain answer: every triangle tested
            let (o64, d64) = ([o.x as f64, o.y as f64, o.z as f64], [d.x as f64, d.y as f64, d.z as f64]);
            let mut g = Gather::new();
            for (i, t) in b.tris.iter().enumerate() {
                g.add(hit(o64, d64, t, SHADOW_TMIN as f64, 5.0), t, i as u32);
            }
            let want = b.decide(&st, &mut g, o64, d64, usize::MAX, 5.0, &mut pw);
            assert_eq!(got, want);
            assert_eq!(&ps.tn[..ps.n], &pw.tn[..pw.n]);
            seen[got as usize] += 1;
            sure += (got != Seen::Unsure) as usize;
        }
        // clear, opaque, through (unsure: rays near an edge, few)
        assert!(seen[0] > 100 && seen[1] > 100 && seen[3] > 100, "{seen:?}");
        assert!(sure > 2000);
    }

    #[test]
    fn shafts_answer_as_the_hierarchy() {
        // the grid of the test above under an array of 24 lights 6 units up (spread 2 x 1) and a second array of
        // suns: every ray of a point answered from its shaft against the hierarchy's answer
        let mut tris = Vec::new();
        for i in 0..40 {
            for j in 0..40 {
                let (x, y) = (-4.0 + 0.2 * i as f32, -4.0 + 0.2 * j as f32);
                let z = 1.0 + ((i * 7 + j * 3) % 20) as f32 * 0.1;
                if (i + j) % 2 == 0 {
                    tris.push(([[x, y, z], [x + 0.15, y, z], [x, y + 0.15, z]], if (i * j) % 3 == 0 { 0x100 } else { 0 }));
                }
            }
        }
        let mut st = state(&tris);
        st.lights.clear();
        let b = Bvh::build(&st.tns, &st.verts, &(0..st.tns.len()).collect::<Vec<_>>()).unwrap();
        for k in 0..24 {
            let (u, v) = ((k as f32 + 0.5) / 24.0 - 0.5, ((k * 7) % 24) as f32 / 24.0 - 0.5);
            st.lights.push(crate::shade::ShadeLight { kind: 2, pos: Vector::new(1.0 + 2.0 * u, v, 6.0), array: 1, ..Default::default() });
        }
        for k in 0..6 {
            let (dx, dy) = (0.05 * k as f32, 0.3 - 0.02 * k as f32);
            let l = (dx * dx + dy * dy + 1.0).sqrt();
            st.lights.push(crate::shade::ShadeLight { kind: 3, dir: Vector::new(dx / l, dy / l, 1.0 / l), array: 2, ..Default::default() });
        }
        let mut seed = 777u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        let mut sh = Shaft::default();
        let (mut seen, mut answered) = ([0usize; 4], 0);
        let (mut ps, mut pw) = (Passes::default(), Passes::default());
        for _ in 0..600 {
            let p = Vector::new(-5.0 + 10.0 * rnd(), -5.0 + 10.0 * rnd(), 0.5 * rnd());
            for li in 0..st.lights.len() {
                let lt = &st.lights[li];
                let (d, dist) = if lt.kind == 3 {
                    (lt.dir, FAR_LIGHT as f32)
                } else {
                    let w = [lt.pos.x - p.x, lt.pos.y - p.y, lt.pos.z - p.z];
                    let l = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
                    (Vector::new(w[0] / l, w[1] / l, w[2] / l), l)
                };
                let got = b.shaft_hit(&mut sh, &st, li, p, d, usize::MAX, SHADOW_TMIN, dist, &mut ps);
                let want = b.any_hit(&st, p, d, usize::MAX, SHADOW_TMIN, dist, &mut pw);
                // the shaft may be sure where the hierarchy's margins left it unsure, never the other way round
                assert!(got == want || want == Seen::Unsure, "{got:?} {want:?}");
                if got == want {
                    assert_eq!(&ps.tn[..ps.n], &pw.tn[..pw.n]);
                }
                seen[got as usize] += 1;
                answered += (!sh.many) as usize;
            }
        }
        // clear, opaque, through (unsure: rays near an edge, few)
        assert!(seen[0] > 100 && seen[1] > 100 && seen[3] > 100, "{seen:?}");
        assert!(answered > 3000, "{answered}");
    }

    /// [`hit`] against the walk's own test (render.rs `probe`, the original's arithmetic in extended precision) on
    /// random triangles at three scales and rays aimed at their corners, edges, just inside and outside, from points on
    /// their plane, nearly along it, with tmin and the light's distance at the hit: Sure only on the walk's hits (its
    /// distance within the bound), Miss only where it misses, Near never beyond a hit.
    #[test]
    fn the_test_answers_as_the_walks() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut u = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut count = [0usize; 3];
        for case in 0..200_000 {
            let scale = [1.0f32, 0.01, 20.0][case % 3];
            let mut pt = |u: &mut dyn FnMut() -> f64| [0, 1, 2].map(|_| (u() * 2.0 - 1.0) as f32 * scale);
            let p = [pt(&mut u), pt(&mut u), pt(&mut u)];
            let (tns, verts) = parts(&[(p, 0)]);
            let Some(b) = Bvh::build(&tns, &verts, &[0]) else { continue };
            let bary = |u: &mut dyn FnMut() -> f64| match (u() * 6.0) as usize {
                0 => 0.0f32,
                1 => 1.0,
                2 => (u() * 1e-6 - 5e-7) as f32,
                3 => (1.0 + u() * 1e-6 - 5e-7) as f32,
                _ => (u() * 1.4 - 0.2) as f32,
            };
            let (ba, bb) = (bary(&mut u), bary(&mut u));
            let bc = 1.0 - ba - bb;
            let target = [0, 1, 2].map(|k| p[0][k] * ba + p[1][k] * bb + p[2][k] * bc);
            let on_plane = u() < 0.15;
            let origin = if on_plane { target } else { [0, 1, 2].map(|k| (u() * 2.0 - 1.0) as f32 * 3.0 * scale) };
            let mut dir = if on_plane { [0, 1, 2].map(|_| (u() * 2.0 - 1.0) as f32) } else { [0, 1, 2].map(|k| target[k] - origin[k]) };
            if u() < 0.1 {
                // nearly along the plane: an edge and a little of the normal
                let (e, n) = (tns[0].edge[(u() * 3.0) as usize % 3], tns[0].n);
                let k = (u() * 2.0 - 1.0) as f32 * 1e-7;
                dir = [e.x + k * n.x, e.y + k * n.y, e.z + k * n.z];
            }
            let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
            if !(l > 0.0) {
                continue;
            }
            let dir = dir.map(|c| c / l);
            let dist = ((0..3).map(|k| ((target[k] - origin[k]) as f64).powi(2)).sum::<f64>()).sqrt();
            let jitter = |u: &mut dyn FnMut() -> f64| (dist * (1.0 + (u() - 0.5) * 1e-6)) as f32;
            let tmin = if u() < 0.2 { jitter(&mut u) } else { SHADOW_TMIN };
            let tmax = if u() < 0.2 { jitter(&mut u) } else { (dist * (1.0 + 3.0 * u())) as f32 + scale };
            let ray = crate::types::Ray { origin: Vector::new(origin[0], origin[1], origin[2]), dir: Vector::new(dir[0], dir[1], dir[2]), t: 1e30, ..Default::default() };
            let walk = crate::render::probe(&ray, &tns, &verts, 0, tmin).filter(|h| h.tf < tmax);
            let (o64, d64) = (origin.map(|c| c as f64), dir.map(|c| c as f64));
            match hit(o64, d64, &b.tris[0], tmin as f64, tmax as f64) {
                Hit::Sure(h, et) => {
                    let w = walk.unwrap_or_else(|| panic!("sure on the walk's miss: {p:?} {ray:?} {tmin:e} {tmax:e}"));
                    assert!((w.t.to_f64() - h).abs() <= et, "{p:?} {ray:?}: {} against {h} within {et:e}", w.t.to_f64());
                    count[0] += 1;
                }
                Hit::Miss => {
                    assert!(walk.is_none(), "miss on the walk's hit: {p:?} {ray:?} {tmin:e} {tmax:e}");
                    count[1] += 1;
                }
                Hit::Near(lo) => {
                    if let Some(w) = walk {
                        assert!(w.t.to_f64() >= lo, "{p:?} {ray:?}");
                    }
                    count[2] += 1;
                }
            }
        }
        eprintln!("sure {}, miss {}, near {}", count[0], count[1], count[2]);
        assert!(count[0] > 10_000 && count[1] > 100_000 && count[2] < count[0] / 4, "{count:?}");
    }

    #[test]
    fn members_come_from_the_file() {
        let dir = std::env::temp_dir().join(format!("softarray-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        load(&dir);
        assert_eq!(group(b"spot1x1"), 0);
        std::fs::write(dir.join(FILE), "spot1x1\nspot1x2\ninf2x1\nspot1x3\n").unwrap();
        load(&dir);
        assert_eq!([group(b"spot1x1"), group(b"spot1x2"), group(b"inf2x1"), group(b"spot1x3"), group(b"spot1")], [1, 1, 2, 1, 0]);
        std::fs::remove_file(dir.join(FILE)).unwrap();
        std::fs::remove_dir(&dir).unwrap();
        load(&dir);
        assert_eq!(group(b"spot1x1"), 0);
    }
}
