// SPDX-License-Identifier: MIT
//! Shadow rays of the add-on's light arrays (unit softspeed1; our own code, nothing here reproduces a function of the
//! original).
//!
//! blend2db writes a Blender light with a size as an array of n lights with ray-traced shadows (unit soft1,
//! re/tools/builder2/blend2db/src/soft.rs) and lists their names in `soft-arrays.txt` beside the database (a file the
//! original never writes or reads). For those lights only, the shadow ray is first answered in doubles (the
//! original's walk tests every triangle listed in every cell the ray crosses: about 3.5 microseconds a shadow ray on the
//! gallery's still life, 80% of its render time with a 49-light array), in two ways:
//! - [`Bvh::shaft_hit`]: the n shadow rays of one shaded point to one array come one after the other, so the triangles
//!   near any of them (near the convex hull of the point and the array's lights, the "shaft") are found once, and each
//!   ray tests only those (4.4 on average on the still life: 79% of its array rays are answered this way);
//! - [`Bvh::any_hit`], for a shaft with too many triangles (a point behind a detailed model) or a ray outside its shaft:
//!   a bounding volume hierarchy (surface-area split) over the triangles of the frame's BSP tree, built once per frame.
//!
//! The answer is used only when it is certain to be the original's: "clear" (no triangle near the segment from the
//! point to the light) or "opaque" (an opaque triangle certainly hit nearer than any triangle that is transparent or
//! within a margin of a hit). Every other ray (a transparent occluder, a ray within the margin of a triangle's edge or
//! of the segment's ends, a light of a selective set) is traced by the original's walk (`trace::s_trace`). The factor
//! is then the one `s_trace` returns: 1, or the umbra. `SOFTARRAY_CHECK=1` walks every ray too and counts: on the still
//! life (480x320, 49 lights) 27.3 million rays, 0 where a certain answer differs from the walk's. What differs: the ray
//! counter and the "met" marks of the triangles the original's walk would have left (neither is read before it is set
//! again), and a transparent triangle's material adjusted by a walk that ended at an opaque one behind it (the original
//! returns the umbra there too).
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
/// candidates, rays of a shaft with too many (per ray), rays leaving their shaft (per ray); shafts built, their candidates
static COUNTS: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];

/// Whether `SOFTARRAY_CHECK` is set to 1.
pub fn checking() -> bool {
    *CHECK.get_or_init(|| std::env::var("SOFTARRAY_CHECK").is_ok_and(|v| v == "1"))
}

/// Counts one ray under [`checking`]: the hierarchy's answer and the factor the walk returned; `umbra` is the light's;
/// the nanoseconds the hierarchy and the walk took.
pub fn record(seen: Seen, walk: [f32; 4], umbra: f32, ns: (u64, u64)) {
    COUNTS[4].fetch_add(ns.0, Ordering::Relaxed);
    COUNTS[5].fetch_add(ns.1, Ordering::Relaxed);
    let k = match seen {
        Seen::Unsure => 3,
        Seen::Clear if walk[..3] == [1.0; 3] => 0,
        Seen::Opaque if walk[..3] == [umbra; 3] => 1,
        _ => 2,
    };
    COUNTS[k].fetch_add(1, Ordering::Relaxed);
}

/// The counts under [`checking`], for render-frames to print: none when not checking.
pub fn report() -> Option<String> {
    checking().then(|| {
        let c = COUNTS.each_ref().map(|c| c.load(Ordering::Relaxed));
        let rays = (c[0] + c[1] + c[2] + c[3]).max(1) as f64;
        format!(
            "softarray check: clear agreed {}, opaque agreed {}, DISAGREED {}, unsure (walked) {}; ns a ray: hierarchy {:.0}, walk {:.0}; \
             shaft rays {}, per ray (many) {}, left the shaft {}; shafts {}, candidates a shaft {:.1}",
            c[0], c[1], c[2], c[3], c[4] as f64 / rays, c[5] as f64 / rays, c[6], c[7], c[8], c[9], c[10] as f64 / c[9].max(1) as f64
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

/// What [`any_hit`] makes of a shadow ray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Clear,
    Opaque,
    Unsure,
}

/// One triangle of the hierarchy: its first corner, two edges, the normal (edge 1 x edge 2), its length, the longest
/// edge, the triangle's index and whether it is opaque (none of the flags 0x340).
struct Tri {
    a: [f64; 3],
    e1: [f64; 3],
    e2: [f64; 3],
    n: [f64; 3],
    nl: f64,
    longest: f64,
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
    /// the largest coordinate (in size) of a corner: the scale of the original's rounding errors
    extent: f64,
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
        Bvh::build(&st.tns, &st.verts, &list).map(std::sync::Arc::new)
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
            let (e1, e2, e3) = (sub(b, a), sub(c, a), sub(c, b));
            let n = cross(e1, e2);
            let longest = dot(e1, e1).max(dot(e2, e2)).max(dot(e3, e3)).sqrt();
            tris.push(Tri { a, e1, e2, n, nl: dot(n, n).sqrt(), longest, tn: i, opaque: t.flags & 0x340 == 0 });
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
        Some(Bvh { nodes, tris, extent })
    }

    /// The margin, a length: 2e-6 of the largest coordinate of the scene or the ray's origin. The original computes
    /// the plane, the distance and the point from floats (6e-8 of their size), so its hit points lie well within it.
    fn margin(&self, o: [f64; 3]) -> f64 {
        2e-6 * (1.0 + o.iter().fold(self.extent, |m, v| m.max(v.abs())))
    }

    /// The shadow ray from `o` along `d` (as `s_trace` gets it: `d` the unit direction to the light, `tmax` its
    /// distance, `tmin` 0.0001, `from` the triangle shaded).
    pub fn any_hit(&self, o: Vector, d: Vector, from: usize, tmin: f32, tmax: f32) -> Seen {
        let o = [o.x as f64, o.y as f64, o.z as f64];
        let d = [d.x as f64, d.y as f64, d.z as f64];
        let (tmin, tmax) = (tmin as f64, tmax as f64);
        if !(o.iter().chain(d.iter()).all(|v| v.is_finite()) && tmax.is_finite() && tmax > tmin) || self.tris.is_empty() {
            return if self.tris.is_empty() && tmax.is_finite() { Seen::Clear } else { Seen::Unsure };
        }
        let e = self.margin(o);
        let inv = d.map(|c| if c.abs() > 1e-12 { 1.0 / c } else { 0.0 });
        let mut t_op = f64::INFINITY; // nearest opaque triangle certainly hit
        let mut t_un = f64::INFINITY; // nearest triangle that is transparent or within the margin of a hit
        // the part of the segment a box must meet: [tmin - e, the nearest answer so far (or tmax) + e]
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
            let t_end = t_op.min(t_un).min(tmax);
            if enter(n, t_end).is_none() {
                continue;
            }
            if n.count > 0 {
                for t in &self.tris[n.first as usize..(n.first + n.count) as usize] {
                    if t.tn == from {
                        continue;
                    }
                    match hit(o, d, t, tmin, tmax, e) {
                        Hit::Miss => {}
                        Hit::Sure(h) if t.opaque => t_op = t_op.min(h),
                        Hit::Sure(h) | Hit::Near(h) => t_un = t_un.min(h),
                    }
                }
                continue;
            }
            if sp + 2 > stack.len() {
                return Seen::Unsure;
            }
            // the nearer child is looked at first
            let (c0, c1) = (n.first, n.first + 1);
            let t_end = t_op.min(t_un).min(tmax);
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
        answer(t_op, t_un)
    }

    /// The shadow ray of light `light` (a member of array `st.lights[light].array`) from `o` along `d`, as
    /// [`Bvh::any_hit`] takes it, answered from `sh`: the shadow rays of one shaded point to the lights of one array
    /// come one after the other with the same origin and triangle, so the triangles near any of them (the "shaft": the
    /// convex hull of the point and the array's lights) are looked up once, and each ray then tests only those. A
    /// shaft with more than [`MAX_CANDIDATES`] (a point behind a detailed model), a point whose lights are spread over
    /// more than 60 degrees, or a ray that leaves its shaft, is answered by [`Bvh::any_hit`].
    #[allow(clippy::too_many_arguments)]
    pub fn shaft_hit(&self, sh: &mut Shaft, st: &ShadeState, light: usize, o: Vector, d: Vector, from: usize, tmin: f32, tmax: f32) -> Seen {
        let group = st.lights[light].array;
        let key = ([o.x.to_bits(), o.y.to_bits(), o.z.to_bits()], from, group);
        let check = checking();
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
            return self.any_hit(o, d, from, tmin, tmax);
        }
        let e = self.margin(o64);
        // the segment the hierarchy looks at, [tmin - e, tmax + e], must lie inside the shaft (widened by m / 2): its
        // ends are looked at, the shaft is convex
        let ends = [d64.map(|c| c * (lo - e)), d64.map(|c| c * (hi + e))];
        if sh.planes.iter().any(|&(n, b)| ends.iter().any(|w| dot(*w, n) > b - 0.5 * sh.m)) {
            if check {
                COUNTS[8].fetch_add(1, Ordering::Relaxed);
            }
            return self.any_hit(o, d, from, tmin, tmax);
        }
        if check {
            COUNTS[6].fetch_add(1, Ordering::Relaxed);
        }
        let (mut t_op, mut t_un) = (f64::INFINITY, f64::INFINITY);
        for &i in &sh.cand {
            let t = &self.tris[i as usize];
            match hit(o64, d64, t, lo, hi, e) {
                Hit::Miss => {}
                Hit::Sure(h) if t.opaque => t_op = t_op.min(h),
                Hit::Sure(h) | Hit::Near(h) => t_un = t_un.min(h),
            }
        }
        answer(t_op, t_un)
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

fn answer(t_op: f64, t_un: f64) -> Seen {
    if t_un.is_finite() && t_un <= t_op {
        Seen::Unsure
    } else if t_op.is_finite() {
        Seen::Opaque
    } else {
        Seen::Clear
    }
}

enum Hit {
    Miss,
    /// certainly a hit at this distance, within the segment
    Sure(f64),
    /// within the margin of a hit (of an edge, of the segment's ends, or nearly parallel): the original's walk decides
    Near(f64),
}

/// Möller and Trumbore's test in doubles, with the margin `e` (a length) widened where the ray meets the plane at a
/// slant: a point within e / cos of an edge, or a distance within e / cos of the segment's ends, is near.
#[inline(always)]
fn hit(o: [f64; 3], d: [f64; 3], t: &Tri, tmin: f64, tmax: f64, e: f64) -> Hit {
    if !(t.nl > 0.0) {
        return Hit::Miss; // no plane: |d . N| is 0, which the original's test takes for a miss
    }
    let pv = cross(d, t.e2);
    let det = dot(t.e1, pv);
    let tv = [o[0] - t.a[0], o[1] - t.a[1], o[2] - t.a[2]];
    let cos = det.abs() / t.nl;
    if cos < 1e-4 {
        // nearly parallel to the plane: a miss when the segment stays well off the plane, else near
        let h0 = dot(t.n, tv) / t.nl;
        let h1 = h0 + tmax * dot(t.n, d) / t.nl;
        return if h0.min(h1) > e || h0.max(h1) < -e { Hit::Miss } else { Hit::Near(tmin) };
    }
    let inv = 1.0 / det;
    let u = dot(tv, pv) * inv;
    let qv = cross(tv, t.e1);
    let v = dot(d, qv) * inv;
    let h = dot(t.e2, qv) * inv;
    let m = u.min(v).min(1.0 - u - v);
    if !(h.is_finite() && m.is_finite()) {
        return Hit::Near(tmin);
    }
    let et = e / cos;
    let ew = 2.0 * et * t.longest / t.nl;
    if m < -ew || h < tmin - et || h > tmax + et {
        return Hit::Miss;
    }
    if m > ew && h > tmin + et && h < tmax - et {
        Hit::Sure(h)
    } else {
        Hit::Near(h.max(tmin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(tris: &[([[f32; 3]; 3], u16)]) -> Bvh {
        let (mut tns, mut verts) = (Vec::new(), Vec::new());
        for (p, flags) in tris {
            let base = verts.len();
            for c in p {
                verts.push(RVertex { world: Vector::new(c[0], c[1], c[2]), ..RVertex::default() });
            }
            tns.push(Tn { v: [Some(base), Some(base + 1), Some(base + 2)], flags: *flags, ..Tn::default() });
        }
        Bvh::build(&tns, &verts, &(0..tns.len()).collect::<Vec<_>>()).unwrap()
    }

    const UP: Vector = Vector { x: 0.0, y: 0.0, z: 1.0 };

    fn tri_at(x: f32, z: f32, flags: u16) -> ([[f32; 3]; 3], u16) {
        ([[x - 1.0, -1.0, z], [x + 1.0, -1.0, z], [x, 1.0, z]], flags)
    }

    #[test]
    fn clear_opaque_and_unsure() {
        let b = scene(&[tri_at(-3.0, 2.0, 0), tri_at(3.0, 2.0, 0x100)]);
        let tmin = SHADOW_TMIN;
        let at = |x: f32, y: f32| Vector::new(x, y, 0.0);
        // nothing above x = 10
        assert_eq!(b.any_hit(at(10.0, 0.0), UP, usize::MAX, tmin, 5.0), Seen::Clear);
        // the opaque triangle above x = -3, the transparent one above x = 3
        assert_eq!(b.any_hit(at(-3.0, 0.0), UP, usize::MAX, tmin, 5.0), Seen::Opaque);
        assert_eq!(b.any_hit(at(3.0, 0.0), UP, usize::MAX, tmin, 5.0), Seen::Unsure);
        // the light below the triangle: clear; the triangle shaded is not looked at
        assert_eq!(b.any_hit(at(-3.0, 0.0), UP, usize::MAX, tmin, 1.5), Seen::Clear);
        assert_eq!(b.any_hit(at(-3.0, 0.0), UP, 0, tmin, 5.0), Seen::Clear);
        // through a corner, or ending at the triangle: the original's walk decides
        assert_eq!(b.any_hit(at(-4.0, -1.0), UP, usize::MAX, tmin, 5.0), Seen::Unsure);
        assert_eq!(b.any_hit(at(-3.0, 0.0), UP, usize::MAX, tmin, 2.0), Seen::Unsure);
    }

    #[test]
    fn the_nearest_decides() {
        let d = {
            let (dx, dz) = (8.0f32, 2.0f32);
            let l = (dx * dx + dz * dz).sqrt();
            Vector::new(dx / l, 0.0, dz / l)
        };
        let o = Vector::new(-5.0, 0.0, 0.0);
        // the ray from x = -5 meets z = 1 at x = -1 and z = 2 at x = 3
        let b = scene(&[tri_at(3.0, 2.0, 0)]);
        assert_eq!(b.any_hit(o, d, usize::MAX, SHADOW_TMIN, 20.0), Seen::Opaque);
        // a transparent triangle in front of the opaque one: unsure; behind it: opaque
        let b = scene(&[tri_at(3.0, 2.0, 0), tri_at(-1.0, 1.0, 0x200)]);
        assert_eq!(b.any_hit(o, d, usize::MAX, SHADOW_TMIN, 20.0), Seen::Unsure);
        let b = scene(&[tri_at(3.0, 2.0, 0), tri_at(7.0, 3.0, 0x200)]);
        assert_eq!(b.any_hit(o, d, usize::MAX, SHADOW_TMIN, 20.0), Seen::Opaque);
    }

    #[test]
    fn many_triangles_agree_with_a_plain_test() {
        // a grid of small triangles over [-4, 4]^2 at heights 1..3, every third transparent; rays up from below,
        // slanted: the hierarchy's answer against testing every triangle
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
        let b = scene(&tris);
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        let (mut seen, mut sure) = ([0usize; 3], 0);
        for _ in 0..4000 {
            let o = Vector::new(-4.0 + 8.0 * rnd(), -4.0 + 8.0 * rnd(), 0.0);
            let (dx, dy) = (rnd() - 0.5, rnd() - 0.5);
            let l = (dx * dx + dy * dy + 1.0).sqrt();
            let d = Vector::new(dx / l, dy / l, 1.0 / l);
            let got = b.any_hit(o, d, usize::MAX, SHADOW_TMIN, 5.0);
            // the plain answer: the nearest triangle hit (by the same test) decides
            let (o64, d64) = ([o.x as f64, o.y as f64, o.z as f64], [d.x as f64, d.y as f64, d.z as f64]);
            let e = b.margin(o64);
            let (mut t_op, mut t_un) = (f64::INFINITY, f64::INFINITY);
            for t in &b.tris {
                match hit(o64, d64, t, SHADOW_TMIN as f64, 5.0, e) {
                    Hit::Miss => {}
                    Hit::Sure(h) if t.opaque => t_op = t_op.min(h),
                    Hit::Sure(h) | Hit::Near(h) => t_un = t_un.min(h),
                }
            }
            assert_eq!(got, answer(t_op, t_un));
            seen[got as usize] += 1;
            sure += (got != Seen::Unsure) as usize;
        }
        assert!(seen.iter().all(|&c| c > 100), "{seen:?}");
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
        let b = scene(&tris);
        let mut st = ShadeState::default();
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
        let (mut seen, mut answered) = ([0usize; 3], 0);
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
                let got = b.shaft_hit(&mut sh, &st, li, p, d, usize::MAX, SHADOW_TMIN, dist);
                let want = b.any_hit(p, d, usize::MAX, SHADOW_TMIN, dist);
                // the shaft may be sure where the hierarchy's margins left it unsure, never the other way round
                assert!(got == want || want == Seen::Unsure, "{got:?} {want:?}");
                seen[got as usize] += 1;
                answered += (!sh.many) as usize;
            }
        }
        assert!(seen.iter().all(|&c| c > 100), "{seen:?}");
        assert!(answered > 3000, "{answered}");
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
