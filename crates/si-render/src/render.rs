// SPDX-License-Identifier: MIT
//! Sampling: the original's `render.c` (finding 0012, "Sampling" and "Intersection").
//!
//! Ported here: the three ray/triangle tests; `SI_Sample`, which turns one sample position
//! into a colour; `antialias`, which filters lines of samples into lines of pixels;
//! `colSubdivid` and `dofSubdivid`, the adaptive subdivision of a pixel; `blur`, the depth of
//! field filter; the two frame loops `SI_sample` and `SI_supersample`; and `checkForExit`.
//! `getRenderLicense`, which both frame loops ask first, is in [`crate::licence`] (without the
//! licence client); on the image the loops' dispatch (`adapter`) asks it before a loop runs.

use crate::mem_chunk::ChunkBuffer;
use crate::renutil::{output, LineSink, OutputState};
use crate::rnd_tag::TagImage;
use crate::types::{RVertex, Ray, Rgbazo, Tn, Vector};
use si_x87::{fx, x, Round, F80};

/// `slice`: the 0x28-byte record that files one triangle (or one half of one) under a scan
/// line. The geometry stage writes +0x04 to +0x1c (see `si_math::geometry::Span`);
/// `SI_Sample` uses the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slice {
    /// +0x00: the sort key `SI_Sample` computes for the current sample: 1 / t, t being the
    /// distance at which the sample's ray meets the triangle's plane.
    pub key: f32,
    /// +0x04, +0x08: left edge on the filing line and its change per line.
    pub x0: f32,
    pub dx0: f32,
    /// +0x0c, +0x10: right edge plus 1 and its change per line.
    pub x1: f32,
    pub dx1: f32,
    /// +0x14: last scan line.
    pub y_end: i32,
    /// +0x18: the triangle.
    pub tn: u32,
    /// +0x1c: next record filed under the same scan line.
    pub next: SliceRef,
    /// +0x20: next record of the list of slices covering the current pixel.
    pub link: SliceRef,
    /// +0x24: next record in depth order, nearest first (written by `SI_Sample`).
    pub by_depth: SliceRef,
}

/// A slice's pointer to another slice: its index in [`Sampler::slices`] plus one, 0 for none.
/// Four bytes where an `Option<usize>` takes sixteen, so a slice takes 40 bytes, as the
/// original's record does: every band thread holds a copy of all the frame's slices
/// ([`crate::bands`]; unit blurmem1). Its `Debug` text is the `Option<usize>`'s.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct SliceRef(u32);

const _: () = assert!(std::mem::size_of::<Slice>() == 40);

impl SliceRef {
    pub const NONE: SliceRef = SliceRef(0);

    /// The slice's index, or `None`.
    #[inline(always)]
    pub fn get(self) -> Option<usize> {
        (self.0 as usize).checked_sub(1)
    }
}

impl From<Option<usize>> for SliceRef {
    #[inline(always)]
    fn from(v: Option<usize>) -> SliceRef {
        match v {
            None => SliceRef(0),
            Some(i) => SliceRef(u32::try_from(i + 1).expect("fewer than 2^32 - 1 slices")),
        }
    }
}

impl std::fmt::Debug for SliceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

/// 0x086542b0, double: a ray whose direction · plane normal is smaller than this in size
/// does not meet the plane (`SI_Intersect` only).
const PARALLEL: f64 = 1e-8;
/// 0x086542bc, float: the distance a ray starts with: farther than any hit.
const NO_HIT: f32 = 100000.0;
/// 0x086542c0, double: added to a hit's distance to start the search for the next layer.
const LAYER_STEP: f64 = 1e-4;

fn corner(verts: &[RVertex], tn: &Tn, k: usize) -> Vector {
    // the original dereferences the corner pointers without a test
    verts[tn.v[k].expect("a filed triangle has three corners")].world
}

/// The three edge functions of the point `p` in the plane perpendicular to the triangle's
/// dominant axis, unrounded: weight k belongs to corner k and is measured from the next
/// corner along the opposite edge. Each is `(p.a − c.a) × e.b − (p.b − c.b) × e.a` with
/// (a, b) = (z, y) for axis 1, (x, z) for axis 2, (y, x) for axis 3.
fn weights(tn: &Tn, verts: &[RVertex], p: [F80; 3]) -> [F80; 3] {
    let w = |c: Vector, e: Vector| match tn.axis {
        1 => fx!((sub (mul (sub (x p[2]) (f c.z)) (f e.y)) (mul (sub (x p[1]) (f c.y)) (f e.z)))),
        2 => fx!((sub (mul (sub (x p[0]) (f c.x)) (f e.z)) (mul (sub (x p[2]) (f c.z)) (f e.x)))),
        _ => fx!((sub (mul (sub (x p[1]) (f c.y)) (f e.x)) (mul (sub (x p[0]) (f c.x)) (f e.y)))),
    };
    [w(corner(verts, tn, 1), tn.edge[0]), w(corner(verts, tn, 2), tn.edge[1]), w(corner(verts, tn, 0), tn.edge[2])]
}

/// The plane normal's component along the dominant axis: the three weights of a point of the
/// triangle sum to it.
fn axis_normal(tn: &Tn) -> f32 {
    match tn.axis {
        1 => tn.n.x,
        2 => tn.n.y,
        _ => tn.n.z,
    }
}

/// Inside when every weight lies between 0 and the normal component (whichever way round
/// they are). The tests are "not below" and "not above" on register values, so a NaN weight
/// counts as inside.
pub(crate) fn inside(w: &[F80; 3], n: f32) -> bool {
    let (lo, hi) = if x(n).gt(x(0.0f32)) { (x(0.0f32), x(n)) } else { (x(n), x(0.0f32)) };
    w.iter().all(|&c| !c.lt(lo) && !c.gt(hi))
}

/// Ray against one triangle [soft 0x0825fc74 SI_Intersect]: `SI_Intersect(ray, tn, exclude,
/// tmin)`. A hit needs: `tn` is not `exclude`; |direction · normal| not below 1e-8; with
/// t = (D − origin · normal) / (direction · normal) in the register, t below the ray's
/// present distance (a NaN passes) and not below `tmin`; and the point origin + float(t) ×
/// direction inside the triangle. On a hit the ray gets t rounded to float, the three
/// weights rounded to float, and the triangle. The original returns 0 for a hit and 2 for a
/// miss; its caller `si_trace` compares the value with 1, which it never is, and goes by
/// whether the ray's distance shrank. This port returns whether it stored. Class: model.
pub fn intersect(ray: &mut Ray, tns: &[Tn], verts: &[RVertex], tn_index: usize, exclude: Option<usize>, tmin: f32) -> bool {
    if exclude == Some(tn_index) {
        return false;
    }
    let tn = &tns[tn_index];
    if certain_miss(ray, tn, verts, tmin) {
        return false;
    }
    intersect_exact(ray, tn, verts, tn_index, tmin)
}

/// 2⁻⁵³, the unit roundoff of a double.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// A test in doubles ahead of [`intersect`]'s exact one, ours (not the original's): true only
/// when the exact test certainly misses, so the answer never changes; false means "not sure"
/// and the exact test decides. Most triangles a traced ray is tested against miss by far (the
/// plane is met outside the cell, or the point lies well outside the triangle), and that is
/// decided here without the x87 unit.
///
/// Why it is safe. With float operands every product here is exact in a double (24 + 24
/// significant bits), and nothing under- or overflows (finite floats give products between
/// 2⁻²⁹⁸ and 2²⁵⁶). So the double result of each of the original's sums differs from the
/// exact real one by at most k·u·(sum of the terms' sizes), k the number of roundings
/// (u = 2⁻⁵³), and the x87 result by at most k·2⁻⁶⁴·(the same): the bounds below take
/// twice and more their sum. The plane distance gets the bound of a quotient; the point is
/// computed from float(t) only when the whole interval of t rounds to the same float (then
/// the original's float(t) is that float: rounding is monotonic, and a double's and an
/// extended's round to nearest even to float are the same function of the real value). A
/// weight is (p_a − c_a)·e_b − (p_b − c_b)·e_a with p = o + float(t)·d: four roundings
/// either way. The miss is then certain when the parallel test, the range test or one of the
/// three corner tests fails by more than its bound. A NaN or infinity anywhere makes the
/// comparisons false: not sure. A triangle without a corner is not sure either, so that the
/// exact test fails as before. `tests::certain_miss_is_a_miss` checks it against the exact
/// test on random and borderline rays.
#[inline(always)]
fn certain_miss(ray: &Ray, tn: &Tn, verts: &[RVertex], tmin: f32) -> bool {
    certain_miss_k(ray, tn, verts, tmin, 1.0)
}

/// [`certain_miss`] with every bound scaled by `k` (1 in the renderer; the tests try others).
#[inline(always)]
fn certain_miss_k(ray: &Ray, tn: &Tn, verts: &[RVertex], tmin: f32, k: f64) -> bool {
    let (o, d, n) = (ray.origin, ray.dir, tn.n);
    let (dx, dy, dz) = (d.x as f64, d.y as f64, d.z as f64);
    let (nx, ny, nz) = (n.x as f64, n.y as f64, n.z as f64);
    // direction · normal: two roundings
    let (a0, a1, a2) = (dz * nz, dy * ny, nx * dx);
    let dn = a0 + (a1 + a2);
    let edn = k * 8.0 * U * (a0.abs() + a1.abs() + a2.abs());
    let adn = dn.abs();
    if adn + edn < PARALLEL * (1.0 - 1e-9) {
        return true;
    }
    if !(adn - edn > PARALLEL * (1.0 + 1e-9)) {
        return false;
    }
    // t = (D − origin · normal) / (direction · normal): three roundings, then the quotient
    let (ox, oy, oz) = (o.x as f64, o.y as f64, o.z as f64);
    let (b0, b1, b2) = (oz * nz, oy * ny, ox * nx);
    let dd = tn.d as f64;
    let num = dd - (b0 + (b1 + b2));
    let enm = k * 8.0 * U * (dd.abs() + b0.abs() + b1.abs() + b2.abs());
    let t = num / dn;
    let et = 2.0 * (enm + t.abs() * edn) / (adn - edn) + k * 8.0 * U * t.abs();
    let (lo, hi) = (t - et, t + et);
    let (rt, tm) = (ray.t as f64, tmin as f64);
    if lo >= rt || hi < tm {
        return true;
    }
    if !(hi < rt && lo >= tm) {
        return false;
    }
    let tf = lo as f32;
    if tf != hi as f32 || !tf.is_finite() {
        return false;
    }
    // the point, one rounding per component
    let tf = tf as f64;
    let p = [ox + tf * dx, oy + tf * dy, oz + tf * dz];
    let (ia, ib) = match tn.axis {
        1 => (2, 1),
        2 => (0, 2),
        _ => (1, 0),
    };
    let nk = axis_normal(tn) as f64;
    let (wlo, whi) = if nk > 0.0 { (0.0, nk) } else { (nk, 0.0) };
    let comps = |v: Vector| [v.x as f64, v.y as f64, v.z as f64];
    let [Some(v0), Some(v1), Some(v2)] = tn.v else { return false };
    for (vi, e) in [(v1, tn.edge[0]), (v2, tn.edge[1]), (v0, tn.edge[2])] {
        let (c, e) = (comps(verts[vi].world), comps(e));
        let (pa, pb) = (p[ia] - c[ia], p[ib] - c[ib]);
        let w = pa * e[ib] - pb * e[ia];
        let ew = k * 16.0 * U * ((p[ia].abs() + c[ia].abs()) * e[ib].abs() + (p[ib].abs() + c[ib].abs()) * e[ia].abs());
        if w + ew < wlo || w - ew > whi {
            return true;
        }
    }
    false
}

/// [`intersect`] from the plane on, as the original computes it.
#[inline(always)]
fn intersect_exact(ray: &mut Ray, tn: &Tn, verts: &[RVertex], tn_index: usize, tmin: f32) -> bool {
    match exact_hit(ray, tn, verts, tmin) {
        Some(h) => {
            ray.t = h.tf;
            ray.hit = h.hit;
            ray.tn = Some(tn_index);
            true
        }
        None => false,
    }
}

/// What [`intersect`] would store for one triangle, with the distance `t` it compared.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    /// The plane distance in the register, as the range tests compare it.
    pub t: F80,
    /// float(t): what the ray gets.
    pub tf: f32,
    /// The three weights rounded to float: what the ray gets.
    pub hit: Vector,
}

/// [`intersect_exact`]'s arithmetic without the store: the hit it would store, if any.
#[inline(always)]
fn exact_hit(ray: &Ray, tn: &Tn, verts: &[RVertex], tmin: f32) -> Option<Hit> {
    let (o, d, n) = (ray.origin, ray.dir, tn.n);
    let dn = fx!((add (mul (f d.z) (f n.z)) (add (mul (f d.y) (f n.y)) (mul (f n.x) (f d.x)))));
    if dn.abs().lt(x(PARALLEL)) {
        return None;
    }
    let t = fx!((div (sub (f tn.d) (add (mul (f o.z) (f n.z)) (add (mul (f o.y) (f n.y)) (mul (f o.x) (f n.x))))) (x dn)));
    if t.ge(x(ray.t)) || t.lt(x(tmin)) {
        return None;
    }
    let tf = t.to_f32();
    let w = weights(tn, verts, [fx!((add (f o.x) (mul (f tf) (f d.x)))), fx!((add (f o.y) (mul (f tf) (f d.y)))), fx!((add (f o.z) (mul (f tf) (f d.z))))]);
    if !inside(&w, axis_normal(tn)) {
        return None;
    }
    Some(Hit { t, tf, hit: Vector::new(w[0].to_f32(), w[1].to_f32(), w[2].to_f32()) })
}

/// [`intersect`] without `exclude` and without the store, for the ray's present distance
/// and `tmin`: the hit it would store, or `None` where it would not. Ours (the shadow walk's
/// replay of a cell, [`crate::trace::s_trace_marked`]); the same tests in the same order.
#[inline(always)]
pub(crate) fn probe(ray: &Ray, tns: &[Tn], verts: &[RVertex], tn_index: usize, tmin: f32) -> Option<Hit> {
    let tn = &tns[tn_index];
    if certain_miss(ray, tn, verts, tmin) {
        return None;
    }
    exact_hit(ray, tn, verts, tmin)
}

/// A ray from the eye against the triangle of one slice [soft 0x0825f118 SI_Intersect1]:
/// `SI_Intersect1(ray, slice, tmin, &tn)`. The distance is not computed from the plane: it is
/// float(1 / key), `key` being what `SI_Sample` stored in the slice; a key of exactly 0 is a
/// miss. The point is that distance times the ray's direction (the origin is taken as the
/// eye, 0). `tmin` is not used. Returns the original's value: 1 for a hit (the ray gets the
/// distance, the weights and the triangle), 2 for a miss; the second value is the slice's
/// triangle, which the original writes through its fourth argument in every case.
/// Class: model.
pub fn intersect1(ray: &mut Ray, slice: &Slice, tns: &[Tn], verts: &[RVertex], _tmin: f32) -> (i32, usize) {
    let t = slice.tn as usize;
    let tn = &tns[t];
    if x(slice.key).eq(x(0.0f32)) {
        return (2, t);
    }
    let tf = fx!(f32: (div (x F80::ONE) (f slice.key)));
    let d = ray.dir;
    let w = weights(tn, verts, [fx!((mul (f tf) (f d.x))), fx!((mul (f tf) (f d.y))), fx!((mul (f tf) (f d.z)))]);
    if !inside(&w, axis_normal(tn)) {
        return (2, t);
    }
    ray.t = tf;
    ray.hit = Vector::new(w[0].to_f32(), w[1].to_f32(), w[2].to_f32());
    ray.tn = Some(t);
    (1, t)
}

/// The same test without the weights [soft 0x0825f6bc SI_Intersect2]: `SI_Intersect2(ray,
/// slice)`. A key that is zero or negative is a miss (a NaN is not). Returns the original's
/// value: 0 for a hit (the ray gets the distance and the triangle only), 2 for a miss.
/// Class: model.
pub fn intersect2(ray: &mut Ray, slice: &Slice, tns: &[Tn], verts: &[RVertex]) -> i32 {
    if x(slice.key).le(x(0.0f32)) {
        return 2;
    }
    let tn = &tns[slice.tn as usize];
    let tf = fx!(f32: (div (x F80::ONE) (f slice.key)));
    let d = ray.dir;
    let w = weights(tn, verts, [fx!((mul (f tf) (f d.x))), fx!((mul (f tf) (f d.y))), fx!((mul (f tf) (f d.z)))]);
    if !inside(&w, axis_normal(tn)) {
        return 2;
    }
    ray.t = tf;
    ray.tn = Some(slice.tn as usize);
    0
}

/// What the sampling routines call outside `render.c`.
pub trait SampleEnv {
    /// The prepared triangles and their corners: what the slices and the rays point to.
    fn geometry(&self) -> (&[Tn], &[RVertex]);
    /// `SI_shade(ray, colour behind, primary)` (0x08263040; the port is `crate::shade::shade`).
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo;
    /// The renderer's chunk memory, `SI_renChunk` (0x0869bc84).
    fn chunk(&mut self) -> &mut ChunkBuffer;
    /// `SI_chunkMalloc(SI_renChunk, size)`. The frame loops allocate their list heads and
    /// line buffers this way and write through the result without testing it; here the data
    /// lives in the [`Sampler`], and the call keeps the arena's books.
    fn chunk_malloc(&mut self, size: i32) -> u32;
}

/// Steps 1 to 4 of [`sample`]: normalises the direction, sorts the pixel's slices by depth
/// and finds the layers. Returns how many of `rays` hold a hit, nearest first.
pub fn sample_layers(slices: &mut [Slice], head: &mut Option<usize>, tns: &[Tn], verts: &[RVertex], dir: Vector, relink: bool, rays: &mut [Ray]) -> usize {
    let mut d = dir;
    let len = fx!((sqrt (add (add (mul (f d.y) (f d.y)) (mul (f d.x) (f d.x))) (mul (f d.z) (f d.z)))));
    if !x(0.0f32).eq(len) {
        d = Vector::new(fx!(f32: (div (f d.x) (x len))), fx!(f32: (div (f d.y) (x len))), fx!(f32: (div (f d.z) (x len))));
    }
    // 2. depth order
    let mut sorted: Option<usize> = None;
    let mut cur = *head;
    while let Some(s) = cur {
        cur = slices[s].link.get();
        let tn = &tns[slices[s].tn as usize];
        let key = fx!((mul (add (add (mul (f d.x) (f tn.n.x)) (mul (f d.y) (f tn.n.y))) (mul (f d.z) (f tn.n.z))) (f tn.inv_d)));
        slices[s].key = key.to_f32();
        let mut before: Option<usize> = None;
        let mut at = sorted;
        while let Some(a) = at {
            if !key.lt(x(slices[a].key)) {
                break;
            }
            before = at;
            at = slices[a].by_depth.get();
        }
        match before {
            Some(b) => slices[b].by_depth = Some(s).into(),
            None => sorted = Some(s),
        }
        slices[s].by_depth = at.into();
    }
    if relink {
        let mut prev = None;
        let mut at = sorted;
        while let Some(a) = at {
            slices[a].link = prev.into();
            prev = at;
            at = slices[a].by_depth.get();
        }
        *head = prev;
    }
    // 4. layers, nearest first
    let mut layers = 0usize;
    let mut at = sorted;
    let mut tmin = 0.0f32;
    loop {
        let ray = &mut rays[layers];
        ray.dir = d;
        ray.t = NO_HIT;
        ray.tn = None;
        let mut hit_tn = None;
        while let Some(a) = at {
            let (r, tn) = intersect1(ray, &slices[a], tns, verts, tmin);
            hit_tn = Some(tn);
            if r == 1 {
                break;
            }
            at = slices[a].by_depth.get();
        }
        if ray.tn.is_none() {
            break;
        }
        layers += 1;
        if layers > 99 || tns[hit_tn.expect("a hit names its triangle")].flags2 == 0 {
            break;
        }
        tmin = (x(ray.t) + LAYER_STEP).to_f32();
        at = slices[at.expect("the hit slice")].by_depth.get();
    }
    layers
}

/// The colour of one sample position [soft 0x082602bc SI_Sample]: `SI_Sample(pixel,
/// &direction, relink, &result, rays)`.
///
/// 0. The fill pointer of the renderer's chunk memory is saved; it is put back at the end,
///    which frees the secondary rays the shading allocated. (When five fill pointers are
///    saved already the save fails and the original puts back whatever its stack held as
///    the mark's number; here that number is 0.)
/// 1. The direction is normalised (left alone when its length is 0).
/// 2. Every slice of the pixel's list (`head`, linked through +0x20) gets its key
///    `(direction · normal) × inv_d` and is inserted into a list linked through +0x24 in
///    falling key order: nearest first. The new key is compared **unrounded** with the stored
///    (float) keys, and goes after equal ones.
/// 3. With `relink` the pixel's own list is rebuilt in the reverse of that order.
/// 4. Layers: a ray (direction, distance 100000, no triangle) is tested against the slices
///    in order with `SI_Intersect1` until one is hit. If the hit triangle's word at +0x52 is
///    non-zero (see the finding: the surface lets what is behind show) and fewer than 100
///    layers are stacked, the next ray of `rays` starts at the slice after the hit one.
/// 5. Starting from `background` (the global at 0x08790cf8), the layers are shaded back to
///    front with `SI_shade(ray, colour so far, 1)`.
///
/// `rays` must hold 100 rays; their origins are not written here (the caller's: the eye).
/// Class: model.
pub fn sample(env: &mut dyn SampleEnv, slices: &mut [Slice], head: &mut Option<usize>, dir: Vector, relink: bool, rays: &mut [Ray], background: Rgbazo) -> Rgbazo {
    let mut mark = 0;
    env.chunk().save_current(Some(&mut mark));
    let layers = {
        let (tns, verts) = env.geometry();
        sample_layers(slices, head, tns, verts, dir, relink, rays)
    };
    // 5. back to front
    let mut colour = background;
    for i in (0..layers).rev() {
        colour = env.shade(&mut rays[i], colour, 1);
    }
    env.chunk().reclaim(mark);
    colour
}

/// The static table `filters` (0x0869cf80): for each sampling level L from 0 to 5 a block of
/// 11 × 11 floats whose top-left (2L − 1)² entries are the product of two triangles,
/// w(i) × w(j) with w(k) = (L − |k − (L − 1)|) / L²: a tent over 2L − 1 samples that sums
/// to 1. `initFilter` copies level L's block to the working filter at 0x0879093c. The
/// table ends at level 5; a level of 6 or more would copy whatever follows it in the
/// binary's data. The values here are computed, and compare equal to the binary's floats
/// (finding 0012 gives the dump).
pub fn filter_for_level(level: i16) -> [[f32; 11]; 11] {
    assert!((0..=5).contains(&level), "the filter table has levels 0 to 5");
    let l = level as i32;
    let mut f = [[0.0f32; 11]; 11];
    let w = |k: i32| (l - (k - (l - 1)).abs()) as f64;
    for (i, row) in f.iter_mut().enumerate().take((2 * l - 1).max(0) as usize) {
        for (j, v) in row.iter_mut().enumerate().take((2 * l - 1) as usize) {
            *v = (w(i as i32) * w(j as i32) / (l * l * l * l) as f64) as f32;
        }
    }
    f
}

/// The state of `antialias`: the working filter, the two rows of pixels being accumulated
/// and their row counters (the file-scope `topBuffer`, `botBuffer`, `fc3`, `fc4`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Antialias {
    /// 0x08790938: 2 × level − 1, the filter's width in samples.
    pub size: i32,
    /// 0x0879093c: the working filter, `[row][column]`.
    pub filter: [[f32; 11]; 11],
    /// `topBuffer`, `botBuffer`: 0x08790b20 (picture width + 10) samples each.
    pub top: Vec<Rgbazo>,
    pub bot: Vec<Rgbazo>,
    /// `fc3`, `fc4`: the filter row the next line of samples has in the top and in the bottom
    /// row of pixels; negative: the line does not count for that row. `SI_sample` starts them
    /// at 0 and −level.
    pub fc3: i32,
    pub fc4: i32,
    /// 0x08790b88: what an accumulating pixel starts from: six floats (0, 0, 0, 0, −100000,
    /// −100000 as `initData` leaves them).
    pub blank: Rgbazo,
}

impl Antialias {
    /// The state as `SI_sample` sets it up for a sampling level above 1.
    pub fn new(level: i16, buffer_len: usize, blank: Rgbazo) -> Antialias {
        // the original fills buffer_len − 1 entries and leaves the last as allocated
        Antialias { size: level as i32 * 2 - 1, filter: filter_for_level(level), top: vec![blank; buffer_len], bot: vec![blank; buffer_len], fc3: 0, fc4: -(level as i32), blank }
    }

    /// Adds one line of samples to the pixel rows it belongs to [soft 0x0825ca00 antialias].
    ///
    /// `line` is the sample buffer at 0x08790930; `xs0..=xs1` the sample columns (0x08790790,
    /// 0x08790794); `xp0` the first pixel column (0x087907a0). Along the line two column
    /// counters run, one starting at 0 and one at −level, each wrapping from the filter's
    /// width to −1 and stepping its pixel by two when it does: so every sample is added to
    /// at most two neighbouring pixels of each of the two rows, with weight
    /// `filter[row counter][column counter]`. Red, green, blue and alpha are summed
    /// (`acc = weight × sample + acc`, rounded to float each time); the depth becomes the
    /// sample's unless the sample's is below it; the sixth value is left alone. After the
    /// line each row counter is stepped; a row whose counter reaches the filter's width is
    /// handed to `output` (`SI_Output`) and reset to [`Antialias::blank`] (all entries but
    /// the last). Class: model.
    pub fn antialias(&mut self, line: &[Rgbazo], xs0: i32, xs1: i32, xp0: i32, output: &mut dyn FnMut(&[Rgbazo])) {
        let (row_top, row_bot) = (self.fc3, self.fc4);
        let (mut c0, mut c1) = (0i16, -((self.size + 1) / 2) as i16);
        let (mut p0, mut p1) = (xp0 as usize, xp0 as usize + 1);
        let add = |acc: &mut Rgbazo, w: f32, s: &Rgbazo| {
            acc.r = (x(w) * s.r + acc.r).to_f32();
            acc.g = (x(w) * s.g + acc.g).to_f32();
            acc.b = (x(w) * s.b + acc.b).to_f32();
            acc.a = (x(acc.a) + x(w) * s.a).to_f32();
            if !x(s.z).lt(x(acc.z)) {
                acc.z = s.z;
            }
        };
        for xs in xs0..=xs1 {
            let s = &line[xs as usize];
            for (row, buf) in [(row_top, &mut self.top), (row_bot, &mut self.bot)] {
                if row >= 0 {
                    if c0 >= 0 {
                        add(&mut buf[p0], self.filter[row as usize][c0 as usize], s);
                    }
                    if c1 >= 0 {
                        add(&mut buf[p1], self.filter[row as usize][c1 as usize], s);
                    }
                }
            }
            c0 += 1;
            if self.size <= c0 as i32 {
                c0 = -1;
                p0 += 2;
            }
            c1 += 1;
            if self.size <= c1 as i32 {
                c1 = -1;
                p1 += 2;
            }
        }
        self.fc3 += 1;
        if self.size <= self.fc3 {
            self.fc3 = -1;
            output(&self.top);
            let n = self.top.len() - 1;
            self.top[..n].fill(self.blank);
        }
        self.fc4 += 1;
        if self.size <= self.fc4 {
            self.fc4 = -1;
            output(&self.bot);
            let n = self.bot.len() - 1;
            self.bot[..n].fill(self.blank);
        }
    }
}

// ------------------------------------------------------------------- adaptive supersampling

/// 0x08654240, 0x08654270, double: the red value that marks an empty entry of the sample
/// cache.
const EMPTY: f64 = -1.0;
/// 0x08654248…0x08654268 (and again at 0x08654278…): a quarter, a half, and the weights of
/// red, green and blue in the contrast test.
const QUARTER: f64 = 0.25;
const HALF: f64 = 0.5;
const LUMA: [f64; 3] = [0.299, 0.587, 0.114];

/// The file-scope state of adaptive supersampling (`SI_supersample`, `colSubdivid`,
/// `dofSubdivid`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Supersample {
    /// 0x08790d3c: sub-samples per pixel side.
    pub sub: i16,
    /// 0x08790922: the line being rendered.
    pub y: i16,
    /// `asBuffer` (0x08791660): the sample cache, `sub + 1` rows of `(x max + 1) × sub + 1`
    /// entries; row 0 is the line's lower edge, row `sub` its upper edge (the lower edge of
    /// the line rendered before). An entry whose red is −1 is empty.
    pub rows: Vec<Vec<Rgbazo>>,
    /// 0x087907c8: the ray direction of sample position (0, 0) and its steps in x and y.
    pub dir: [f32; 5],
    /// `resort` (0x08791674): slices joined the pixel's list at this pixel, so `SI_Sample`
    /// rebuilds the list in depth order.
    pub resort: i16,
    /// `winner` (0x087916a0): the 100 layer rays.
    pub winner: Vec<Ray>,
    /// 0x08790d4c: the contrast above which a corner is refined.
    pub contrast: f32,
    /// 0x08790b24: `dofSubdivid` refines only where the depth of field value is below this
    /// in size.
    pub dof_limit: f32,
    /// 0x08790cf8: the background.
    pub background: Rgbazo,
    /// Not in the original: what `colSubdivid` returns as the sixth value, a stack word the
    /// original never sets (0 by default; see `ShadeGlobals::unset`).
    pub unset: f32,
}

impl Supersample {
    /// The cache entry of sample position (`xf`, `yf`), filled by `SI_Sample` when empty.
    fn corner(&mut self, env: &mut dyn SampleEnv, slices: &mut [Slice], head: &mut Option<usize>, ix: i32, iy: i32, xf: f32, yf: f32) -> Rgbazo {
        // the original indexes its rows with whatever the conversions gave
        let (r, c) = (iy as usize, ix as usize);
        if x(self.rows[r][c].r).eq(x(EMPTY)) {
            let dir = Vector::new((x(xf) * self.dir[3] + self.dir[0]).to_f32(), (x(yf) * self.dir[4] + self.dir[1]).to_f32(), self.dir[2]);
            self.rows[r][c] = sample(env, slices, head, dir, self.resort != 0, &mut self.winner, self.background);
        }
        self.rows[r][c]
    }

    /// The four corners of a rectangle, in the order the original fetches them: (x0, y1),
    /// (x1, y1), (x0, y0), (x1, y0); returned as [C (x0, y0), D (x1, y0), A (x0, y1),
    /// B (x1, y1)] with the two row indices.
    #[allow(clippy::too_many_arguments)]
    fn corners(&mut self, env: &mut dyn SampleEnv, slices: &mut [Slice], head: &mut Option<usize>, x0: f32, x1: f32, y0: f32, y1: f32) -> ([Rgbazo; 4], i32, i32) {
        let s = x(self.sub as i32);
        let ix0 = (x(x0) * s).to_i32(Round::Zero);
        let ix1 = (x(x1) * s).to_i32(Round::Zero);
        let line = x(self.y as i32);
        let iy0 = ((x(y0) - line) * s).to_i32(Round::Zero);
        let iy1 = ((x(y1) - line) * s).to_i32(Round::Zero);
        let a = self.corner(env, slices, head, ix0, iy1, x0, y1);
        let b = self.corner(env, slices, head, ix1, iy1, x1, y1);
        let c = self.corner(env, slices, head, ix0, iy0, x0, y0);
        let d = self.corner(env, slices, head, ix1, iy0, x1, y0);
        ([c, d, a, b], iy0, iy1)
    }
}

/// The mean of four samples as both subdivision routines form it: red, green and blue
/// `(((C + D) + A) + B) × 0.25` in the register, rounded to float; the depth the greatest
/// of the four (each replaces the running value unless it is below it, so a NaN replaces
/// it). The sixth value is left as given.
fn mean_of(q: &[Rgbazo; 4], o: f32) -> Rgbazo {
    let [c, d, a, b] = q;
    let m = |f: fn(&Rgbazo) -> f32| ((((x(f(c)) + f(d)) + f(a)) + f(b)) * QUARTER).to_f32();
    let mut z = c.z;
    for v in [d.z, a.z, b.z] {
        if !x(v).lt(x(z)) {
            z = v;
        }
    }
    Rgbazo { r: m(|p| p.r), g: m(|p| p.g), b: m(|p| p.b), a: m(|p| p.a), z, o }
}

/// The contrast of a corner against the mean: `float((0.299 Δr + 0.587 Δg) + 0.114 Δb)`,
/// above the threshold in size.
fn contrast_above(p: &Rgbazo, mean: &Rgbazo, threshold: f32) -> bool {
    let e = (((x(p.r) - mean.r) * LUMA[0] + (x(p.g) - mean.g) * LUMA[1]) + (x(p.b) - mean.b) * LUMA[2]).to_f32();
    x(e).abs().gt(x(threshold))
}

/// `|v| < limit` as `dofSubdivid` tests it: the sign is looked at first, a NaN fails.
fn below_in_size(v: f32, limit: f32) -> bool {
    if x(v).lt(x(0.0f32)) || x(v).unordered(x(0.0f32)) {
        (-x(v)).lt(x(limit))
    } else {
        x(limit).gt(x(v))
    }
}

/// The value of the rectangle x0..x1 × y0..y1 (in pixels) by adaptive subdivision
/// [soft 0x0825dc38 colSubdivid]: `colSubdivid(pixel, x0, x1, y0, y1)`.
///
/// The four corners come from the sample cache (index: trunc(x × sub) and trunc((y − line)
/// × sub)), each filled by `SI_Sample` at that exact position when its red is −1. The result
/// is their mean (see `mean_of`). If the rectangle is more than one cache row high, each
/// corner whose contrast against that mean is above the threshold is replaced by the value
/// of its quadrant (the same routine on half the rectangle; the mean used for the four
/// tests is the first one throughout), and if any was, the mean is formed again.
///
/// **The sixth value of the result is never written by the original** (a stack word): here
/// it is `unset`. `SI_supersample` stores the whole result in the line buffer; without depth
/// of field nothing reads that value. Class: model.
pub fn col_subdivid(ss: &mut Supersample, env: &mut dyn SampleEnv, slices: &mut [Slice], head: &mut Option<usize>, x0: f32, x1: f32, y0: f32, y1: f32) -> Rgbazo {
    let (mut q, iy0, iy1) = ss.corners(env, slices, head, x0, x1, y0, y1);
    let mut mean = mean_of(&q, ss.unset);
    if iy1.wrapping_sub(iy0) > 1 {
        let xm = ((x(x0) + x1) * HALF).to_f32();
        let ym = ((x(y0) + y1) * HALF).to_f32();
        let quads = [(x0, xm, y0, ym), (xm, x1, y0, ym), (x0, xm, ym, y1), (xm, x1, ym, y1)];
        let mut any = false;
        for k in 0..4 {
            if contrast_above(&q[k], &mean, ss.contrast) {
                let (a, b, c, d) = quads[k];
                q[k] = col_subdivid(ss, env, slices, head, a, b, c, d);
                any = true;
            }
        }
        if any {
            mean = mean_of(&q, ss.unset);
        }
    }
    mean
}

/// The same with depth of field [soft 0x0825e340 dofSubdivid]: `dofSubdivid(pixel, x0, x1,
/// y0, y1)`. As `colSubdivid`, and in addition:
///
/// - the sixth value of the result is the least of the four corners' (each replaces the
///   running value unless it is above it); **it is not formed again after a refinement**;
/// - nothing is refined unless that least value is below the limit at 0x08790b24 in size;
/// - a corner is refined only if its own sixth value is below that limit in size, as well
///   as its contrast being above the threshold.
///
/// Class: model.
pub fn dof_subdivid(ss: &mut Supersample, env: &mut dyn SampleEnv, slices: &mut [Slice], head: &mut Option<usize>, x0: f32, x1: f32, y0: f32, y1: f32) -> Rgbazo {
    let (mut q, iy0, iy1) = ss.corners(env, slices, head, x0, x1, y0, y1);
    let mut o = q[0].o;
    for v in [q[1].o, q[2].o, q[3].o] {
        if !x(v).gt(x(o)) {
            o = v;
        }
    }
    let mut mean = mean_of(&q, o);
    if iy1.wrapping_sub(iy0) > 1 && below_in_size(o, ss.dof_limit) {
        let xm = ((x(x0) + x1) * HALF).to_f32();
        let ym = ((x(y0) + y1) * HALF).to_f32();
        let quads = [(x0, xm, y0, ym), (xm, x1, y0, ym), (x0, xm, ym, y1), (xm, x1, ym, y1)];
        let mut any = false;
        for k in 0..4 {
            if contrast_above(&q[k], &mean, ss.contrast) && below_in_size(q[k].o, ss.dof_limit) {
                let (a, b, c, d) = quads[k];
                q[k] = dof_subdivid(ss, env, slices, head, a, b, c, d);
                any = true;
            }
        }
        if any {
            mean = mean_of(&q, o);
        }
    }
    mean
}

// --------------------------------------------------------------------------- depth of field

/// 0x086542a0, float: "no value" for a depth and for the sixth value of a sample.
const NONE: f32 = -100000.0;
/// 0x086542a8, double: a neighbour spreads its blur onto a pixel only where the widest
/// filter's weight is above this.
const SPREAD_WEIGHT: f64 = 0.5;
/// Not in the original: columns of one depth of field line per piece handed to a thread, and
/// the least work (columns × lines²) for which a line is filtered on several threads.
const PAR_COLUMNS: usize = 32;
const PAR_WORK: usize = 1_000_000;

/// The state of the depth of field filter `blur`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DofBlur {
    /// 0x08790b28: number of lines kept (odd, at least 5).
    pub lines: i32,
    /// 0x08790b2c: half of it: the widest blur radius, in samples.
    pub half: i32,
    /// 0x08790b30: the radius used for a sample that carries no depth of field value.
    pub radius: i32,
    /// 0x087906d0: the depth at which the blur is widest (negative).
    pub z2: f32,
    /// 0x08790b38: one filter per radius, `[radius][row][column]`, each row and column
    /// index counted from the window's corner (`makeFilter` builds them; the centre is at
    /// `half`).
    pub filters: Vec<Vec<Vec<f32>>>,
    /// 0x08790b34: the last `lines` lines of samples, oldest first.
    pub rows: Vec<Vec<Rgbazo>>,
    /// `nCall` (0x08791670): lines taken in so far.
    pub n_call: i32,
}

impl DofBlur {
    /// Takes one line of samples in and, once `lines` lines are held, replaces `line` with
    /// the filtered middle line: the body of `blur` [soft 0x0825eb98 blur] up to its last
    /// step, which hands the line on (see [`Sampler::blur`]). Returns whether `line` now
    /// holds a filtered line.
    ///
    /// 1. The rows move up by one and the new line's samples xs0 .. xs1 − 1 are copied into
    ///    the last row.
    /// 2. Until `lines` lines have come in, nothing more happens.
    /// 3. `line[xs0 .. xs1 − 1]` is reset to `blank`. Then for x from xs0 + 1 + half up to
    ///    xs1 − half − 1, the sample S = rows\[half\]\[x\] gives output entry
    ///    xs0 + (x − (xs0 + 1 + half)):
    ///    - v = S's sixth value. If v ≤ 0: m = the greatest of w × (neighbour's sixth
    ///      value) over every third row and column of the widest filter where the weight w
    ///      is above 0.5; if m > 0 and −v < m, v = m (kept unrounded).
    ///    - radius = the default (0x08790b30) if v is −100000, else trunc((−|v| / z2) ×
    ///      half).
    ///    - radius ≤ 0: S is copied. Otherwise, radius held to `half`: colour and alpha
    ///      start from S's with weight 1, and every neighbour (row j, column x + i − half,
    ///      i and j from half − radius to half + radius) is added with weight
    ///      filter\[radius\]\[j\]\[i\] unless: v is exactly 0 and the neighbour's sixth value is
    ///      not above 0; or the neighbour's is −100000 and v is above both S's own and 0;
    ///      or the weight is 0. Each channel is `float(w × c + sum)`; the weights are summed
    ///      unrounded. The result is `float(1 / weights) × sum`, the depth the greatest of
    ///      −100000 and **every** neighbour's depth in the window, skipped or not; the
    ///      sixth value stays `blank`'s.
    ///
    /// Class: model.
    pub fn filter_line(&mut self, line: &mut [Rgbazo], xs0: i32, xs1: i32, blank: Rgbazo) -> bool {
        let (x0, x1) = (xs0 as i16, xs1 as i16);
        // 1.
        let last = (self.lines as i16).wrapping_sub(1);
        if last > 0 {
            self.rows[..=last as usize].rotate_left(1);
        }
        // (with one line or none the original still stores row 0 at index `last`)
        let newest = last as isize as usize;
        if x0 < x1 {
            for i in x0 as usize..x1 as usize {
                self.rows[newest][i] = line[i];
            }
        }
        // 2.
        self.n_call = self.n_call.wrapping_add(1);
        if self.n_call < self.lines {
            return false;
        }
        // 3.
        if x0 < x1 {
            for p in &mut line[x0 as usize..x1 as usize] {
                *p = blank;
            }
        }
        let first = x0.wrapping_add(1).wrapping_add(self.half as i16);
        let end = x1.wrapping_sub(self.half as i16);
        let count = if first < end { (end as i32 - first as i32) as usize } else { 0 };
        if count == 0 {
            return true;
        }
        let dst = xs0 as usize;
        let out = &mut line[dst..dst + count];
        let threads = crate::bands::threads();
        let n = self.lines.max(0) as usize;
        if threads > 1 && count >= 2 * PAR_COLUMNS && count.saturating_mul(n * n) >= PAR_WORK {
            // Not in the original: the columns of one line on several threads
            // (`RENDER_THREADS`). Each output entry is a function of the rows and the filters
            // alone ([`DofBlur::column`]), written once, so the line is the same bits.
            let parts = std::sync::Mutex::new(out.chunks_mut(PAR_COLUMNS).enumerate());
            let me = &*self;
            std::thread::scope(|sc| {
                for _ in 0..threads.min(count.div_ceil(PAR_COLUMNS)) {
                    sc.spawn(|| loop {
                        let next = parts.lock().map(|mut it| it.next()).unwrap_or(None);
                        let Some((k, part)) = next else { break };
                        let c0 = first as i32 + (k * PAR_COLUMNS) as i32;
                        for (i, p) in part.iter_mut().enumerate() {
                            *p = me.column(c0 + i as i32, blank);
                        }
                    });
                }
            });
        } else {
            for (i, p) in out.iter_mut().enumerate() {
                *p = self.column(first as i32 + i as i32, blank);
            }
        }
        true
    }

    /// The filtered entry for column `col` of the middle row (step 3 of
    /// [`DofBlur::filter_line`]): the sample itself when its radius is 0 or less, else
    /// `blank` with the filtered colour, alpha and depth.
    fn column(&self, col: i32, blank: Rgbazo) -> Rgbazo {
        let n = self.lines;
        let src = self.rows[self.half as usize][col as usize];
        let mut v = x(src.o);
        if v.le(x(0.0f32)) {
            let mut m = x(NONE);
            let wide = &self.filters[self.half as usize];
            let mut i: i16 = 0;
            while (i as i32) < n {
                let c = col + i as i32 - self.half;
                let mut j: i16 = 0;
                while (j as i32) < n {
                    let w = x(wide[j as usize][i as usize]);
                    if w.gt(x(SPREAD_WEIGHT)) {
                        let t = w * self.rows[j as usize][c as usize].o;
                        if t.gt(m) {
                            m = t;
                        }
                    }
                    j = j.wrapping_add(3);
                }
                i = i.wrapping_add(3);
            }
            if m.gt(x(0.0f32)) && (-x(src.o)).lt(m) {
                v = m;
            }
        }
        let mut radius = self.radius;
        if !v.eq(x(NONE)) {
            let a = if v.lt(x(0.0f32)) || v.unordered(x(0.0f32)) { v } else { -v };
            radius = ((a / x(self.z2)) * x(self.half)).to_i32(Round::Zero);
        }
        if radius <= 0 {
            return src;
        }
        if radius > self.half {
            radius = self.half;
        }
        let (lo, hi) = (self.half - radius, radius + self.half);
        let filter = &self.filters[radius as usize];
        // the parts of the skip test that depend on v alone, taken once
        let v_zero = v.eq(x(0.0f32));
        let v_above = v.gt(x(src.o)) && v.gt(x(0.0f32));
        let mut acc = [src.r, src.g, src.b, src.a];
        let mut z = NONE;
        let mut weights = F80::ONE;
        let mut i = lo as i16;
        while i as i32 <= hi {
            let c = (col + i as i32 - self.half) as usize;
            let mut j = lo as i16;
            while j as i32 <= hi {
                let p = self.rows[j as usize][c];
                // The comparisons of two floats are made on the floats themselves: widening a
                // float to 80 bits is exact, keeps the order and keeps NaN unordered and
                // −0 equal to +0, so each test has the unit's answer.
                if !(p.z < z) {
                    z = p.z;
                }
                let skip = (v_zero && !(0.0f32 < p.o)) || (p.o == NONE && v_above);
                if !skip {
                    let w = filter[j as usize][i as usize];
                    if w != 0.0f32 {
                        // float(w × c + sum), one block on the unit each (the operators' answer)
                        acc[0] = fx!(f32: (add (mul (f w) (f p.r)) (f acc[0])));
                        acc[1] = fx!(f32: (add (mul (f w) (f p.g)) (f acc[1])));
                        acc[2] = fx!(f32: (add (mul (f w) (f p.b)) (f acc[2])));
                        acc[3] = fx!(f32: (add (mul (f w) (f p.a)) (f acc[3])));
                        weights = fx!((add (x weights) (f w)));
                    }
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let k = (F80::ONE / weights).to_f32();
        let mut out = blank;
        out.r = (x(k) * acc[0]).to_f32();
        out.g = (x(k) * acc[1]).to_f32();
        out.b = (x(k) * acc[2]).to_f32();
        out.a = (x(k) * acc[3]).to_f32();
        out.z = z;
        out
    }
}

// --------------------------------------------------------------------------- the frame loops

/// What the frame loops call outside `render.c`, beyond [`SampleEnv`] and the destinations
/// of a finished line ([`LineSink`]).
pub trait FrameEnv: SampleEnv + LineSink {
    /// `SI_displayStatus(text)`.
    fn display_status(&mut self, text: &str);
    /// `SI_cusCheckRenderAbort()`: lets a custom effect stop the render.
    fn check_render_abort(&mut self);
    /// `checkForExit()` (see [`check_for_exit`]); called when the program is not the
    /// standalone renderer.
    fn check_for_exit(&mut self);
    /// `fopen(name, "wb+")` for the depth file `<output name>.<frame>.Zpic`; the result
    /// goes to `zFp` (0x087932c0) untested.
    fn depth_open(&mut self, name: &[u8]);
    /// `fclose(zFp)`.
    fn depth_close(&mut self);
    /// `si_tagWriteScanline(tags->file, tags, 0)`.
    fn tag_write_scanline(&mut self, tags: &TagImage);
}

/// The frame as the two loops see it: the set-up `initData`, `SI_camera`, `si_setBounds`
/// and the geometry stage leave in `SI_database` and `SI_ren`, and the file-scope variables
/// of `render.c`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampler {
    /// 0x08790d38: sampling level L.
    pub level: i16,
    /// 0x08790790…9c: the rendered region in samples: x min, x max, y min, y max.
    pub region: [i32; 4],
    /// 0x087907a8, 0x087907ac: its first and last line in pixels.
    pub region_y_px: [i32; 2],
    /// 0x08790788, 0x0879078c: the picture's first and last line.
    pub picture_y: [f32; 2],
    /// 0x087907b0, 0x087907b4: the window's first and last sample column.
    pub window_x: [f32; 2],
    /// The adaptive sampler's state; its `dir`, `background`, `y` and `winner` serve the
    /// regular sampler too (0x087907c8, 0x08790cf8, 0x08790922).
    pub ss: Supersample,
    /// 0x08790920: the sample column being rendered.
    pub x: i16,
    /// 0x08790d14: the background as four bytes.
    pub background_bytes: [u8; 4],
    /// 0x08790b88: the blank sample.
    pub blank: Rgbazo,
    /// 0x08790b20: length of the pixel rows of `antialias`.
    pub buffer_len: i32,
    /// 0x087906ac: depth of field on.
    pub dof_on: i32,
    pub dof: DofBlur,
    /// 0x08790e04, 0x08790e0c, 0x08790e18, 0x08790e1c: crop on, its x and y offsets (floats)
    /// and its width.
    pub crop: i32,
    pub crop_x: f32,
    pub crop_y: f32,
    pub crop_width: i16,
    /// 0x08790e22: a resumed picture: the lines above the region are in the file already.
    pub resume: i16,
    /// 0x08790d1c, 0x08790d20: field rendering and the current field.
    pub field: i16,
    pub field_no: i16,
    /// `SI_standaloneRenderer` (0x08679400).
    pub standalone: i16,
    /// 0x08790e20: tag output.
    pub tags_on: i16,
    /// 0x08790d5c: the output name.
    pub name: Vec<u8>,
    /// 0x087908ec: per sample line, the head of the slices that start there.
    pub scan_lines: Vec<Option<usize>>,
    /// 0x087908f0: per sample column, the slices of the line that start there.
    pub columns: Vec<Option<usize>>,
    pub slices: Vec<Slice>,
    /// 0x08790930: the line of samples.
    pub line: Vec<Rgbazo>,
    /// The working filter and the pixel rows of `antialias`.
    pub aa: Antialias,
    /// `srXRes`, `srXOffset`, `srYOffset` (0x0879167c, 0x08791678, 0x08791680): what
    /// `SI_Output` is given.
    pub sr_x_res: i16,
    pub sr_x_offset: i32,
    pub sr_y_offset: i32,
    /// The line buffers and switches of `SI_Output`; `out.y` is the line counter
    /// 0x08790b3c, `out.width` the picture width 0x0879117a, `out.frame` 0x08790dda.
    pub out: OutputState,
    /// `SI_tags` (0x08791224).
    pub tags: TagImage,
    /// Per model element index (`tn` +0x58): the tag channels the model belongs to (model
    /// +0x358 count, +0x35c list).
    pub model_tags: Vec<Vec<i32>>,
}

impl Sampler {
    /// `srXRes`, `srXOffset` and `srYOffset` as both loops set them first: the whole width,
    /// or the crop's width and its two offsets truncated.
    fn set_crop(&mut self) {
        if self.crop != 0 {
            self.sr_x_res = self.crop_width;
            self.sr_x_offset = x(self.crop_x).to_i32(Round::Zero);
            self.sr_y_offset = x(self.crop_y).to_i32(Round::Zero);
        } else {
            self.sr_x_res = self.out.width;
            self.sr_x_offset = 0;
            self.sr_y_offset = 0;
        }
    }

    fn begin_message(&self, env: &mut dyn FrameEnv) {
        if self.standalone != 0 {
            let text = if self.field != 0 { format!("begin to render frame {}.{} ...\n", self.out.frame, self.field_no as i32 + 1) } else { format!("begin to render frame {}...\n", self.out.frame) };
            env.display_status(&text);
        }
    }

    /// Opens `<name>.<frame>.Zpic` and fills the depth line with −100000.
    fn depth_begin(&mut self, env: &mut dyn FrameEnv) {
        let mut name = self.name.clone();
        name.extend_from_slice(format!(".{}.Zpic", self.out.frame).as_bytes());
        env.depth_open(&name);
        for d in self.out.depth.iter_mut().take(self.out.width.max(0) as usize) {
            *d = NONE;
        }
    }

    /// One line of background: the four outgoing byte lines are filled and sent to the
    /// incrustation, the picture file and the display.
    fn background_line(&mut self, env: &mut dyn FrameEnv, y: i16) {
        self.out.y = y;
        let w = self.out.width.max(0) as usize;
        let b = self.background_bytes;
        self.out.line.red[..w].fill(b[0]);
        self.out.line.green[..w].fill(b[1]);
        self.out.line.blue[..w].fill(b[2]);
        self.out.line.alpha[..w].fill(b[3]);
        if self.out.incrust {
            env.incrustation(&mut self.out.line, self.out.width as i32);
        }
        if self.out.to_file {
            env.rle_write(self.out.y as i32, self.out.width as i32, &self.out.line);
        }
        if self.out.display {
            env.fb_write(self.out.y);
        }
    }

    /// The lines above the rendered region, top line first; not for a crop or a resumed
    /// picture. Each is background, with a depth line (as the buffer stands) and a tag line.
    fn lines_above(&mut self, env: &mut dyn FrameEnv) {
        if self.crop != 0 || self.resume != 0 {
            return;
        }
        let mut y = x(self.picture_y[1]).to_i32(Round::Zero) as i16;
        while y as i32 > self.region_y_px[1] {
            self.background_line(env, y);
            if self.out.depth_file {
                env.depth_write(&self.out.depth[..self.out.width.max(0) as usize]);
            }
            if self.tags_on != 0 {
                env.tag_write_scanline(&self.tags);
            }
            y = y.wrapping_sub(1);
        }
    }

    /// Active list and column table for sample line `y`: slices that end above the line
    /// are dropped, the others step their two edges (`x += dx`, rounded to float); the
    /// slices filed under the line join at the tail, unstepped. Then every column entry up
    /// to the window's last column is cleared and each active slice is put at the head of
    /// the column trunc(x0), x0 held to the window (a NaN is not held, and truncates to
    /// column 0 through the 16-bit index).
    pub(crate) fn start_line(&mut self, active: &mut Option<usize>, y: i16) {
        let mut prev: Option<usize> = None;
        let mut cur = *active;
        while let Some(s) = cur {
            let next = self.slices[s].next.get();
            if self.slices[s].y_end > y as i32 {
                match prev {
                    Some(p) => self.slices[p].next = next.into(),
                    None => *active = next,
                }
            } else {
                let sl = &mut self.slices[s];
                sl.x0 = (x(sl.x0) + sl.dx0).to_f32();
                sl.x1 = (x(sl.x1) + sl.dx1).to_f32();
                prev = Some(s);
            }
            cur = next;
        }
        // (a negative line number reads before the table in the original, a line past its
        // end the column table after it: Rust differs, such a line has nothing filed;
        // `update` lengthens the table to the grown window, so a frame does not get here)
        let filed = usize::try_from(y as i32).ok().and_then(|i| self.scan_lines.get(i).copied()).flatten();
        match prev {
            Some(p) => self.slices[p].next = filed.into(),
            None => *active = filed,
        }
        let mut i: i16 = 0;
        while x(i as i32).le(x(self.window_x[1])) {
            self.columns[i as usize] = None;
            i = i.wrapping_add(1);
        }
        let mut cur = *active;
        while let Some(s) = cur {
            let x0 = self.slices[s].x0;
            let v = if x(self.window_x[0]).gt(x(x0)) {
                self.window_x[0]
            } else if x(self.window_x[1]).lt(x(x0)) {
                self.window_x[1]
            } else {
                x0
            };
            let col = x(v).to_i32(Round::Zero) as i16 as usize;
            self.slices[s].link = self.columns[col].into();
            self.columns[col] = Some(s);
            cur = self.slices[s].next.get();
        }
    }

    /// The pixel list at sample column `xs`: slices whose right edge (trunc(x1)) lies left
    /// of the column leave, the column's own slices join at the tail.
    pub(crate) fn start_column(&mut self, pixel: &mut Option<usize>, xs: i16) {
        let mut prev: Option<usize> = None;
        let mut cur = *pixel;
        while let Some(s) = cur {
            let next = self.slices[s].link.get();
            if x(self.slices[s].x1).to_i32(Round::Zero) < xs as i32 {
                match prev {
                    Some(p) => self.slices[p].link = next.into(),
                    None => *pixel = next,
                }
            } else {
                prev = Some(s);
            }
            cur = next;
        }
        let joined = self.columns[xs as i32 as usize];
        match prev {
            Some(p) => self.slices[p].link = joined.into(),
            None => *pixel = joined,
        }
    }

    /// Sets the tag bit of pixel `px` in every channel the model of triangle `tn` belongs
    /// to (line 0 of the tag image).
    fn tag_pixel(&mut self, env: &dyn SampleEnv, tn: usize, px: i32) {
        let model = env.geometry().0[tn].w58 as usize;
        for k in 0..self.model_tags[model].len() {
            let channel = self.model_tags[model][k] as usize;
            self.tags.data[channel][0][(px >> 3) as usize] |= 1u8 << (!px & 7);
        }
    }

    /// Writes the tag line and clears the bits of every pixel of the picture's width in
    /// every channel.
    fn tag_line(&mut self, env: &mut dyn FrameEnv) {
        env.tag_write_scanline(&self.tags);
        for px in 0..self.out.width as i32 {
            for k in 0..self.tags.channels.max(0) as usize {
                self.tags.data[k][0][(px >> 3) as usize] &= 0xfeu8.rotate_left((7 - (px & 7)) as u32);
            }
        }
    }

    /// The depth of field filter [soft 0x0825eb98 blur]: `blur()`. Takes the line of
    /// samples in ([`DofBlur::filter_line`]) and, once enough lines are held, hands the
    /// filtered line to `antialias` (level above 1) or to `SI_Output`.
    pub fn blur(&mut self, sink: &mut dyn LineSink) {
        if self.dof.filter_line(&mut self.line, self.region[0], self.region[1], self.blank) {
            if self.level > 1 {
                self.antialias(sink);
            } else {
                output(&mut self.out, sink, &self.line, self.sr_x_offset, self.sr_y_offset, self.sr_x_res as i32);
            }
        }
    }

    /// `antialias()` on the line of samples, its finished rows going to `SI_Output`.
    fn antialias(&mut self, sink: &mut dyn LineSink) {
        let (out, sx, sy, sres) = (&mut self.out, self.sr_x_offset, self.sr_y_offset, self.sr_x_res as i32);
        let xp0 = out.x0;
        self.aa.antialias(&self.line, self.region[0], self.region[1], xp0, &mut |row| output(out, sink, row, sx, sy, sres));
    }

    /// Renders the frame on a regular grid of samples [soft 0x0825ba18 SI_sample]:
    /// `SI_sample()`.
    ///
    /// The original first calls `getRenderLicense` ([`crate::licence::get_render_license`])
    /// and returns at once when that gives 0; on the image that call is made by the loop's
    /// dispatch (`adapter`), and this is the loop after a granted licence.
    ///
    /// Set-up: the standalone renderer's status line; the crop values for `SI_Output`;
    /// three list heads from chunk memory (active slices, the pixel's slices, the depth
    /// order: the last lives inside [`sample_layers`] here); 100 layer rays from the eye
    /// (origin 0, depth index + 1); for a level above 1 the two pixel rows of `antialias`
    /// with their row counters 0 and −level; for depth of field `lines` rows of x max + 10
    /// samples (the first x max of each blank) and a line count of 0, and the first ray's
    /// column and line moved back by `half`; with a depth file the file is opened and the
    /// depth line set to −100000. Then the lines above the region are written as background.
    ///
    /// Per sample line y from y max down to y min: abort checks; a chunk mark; the active
    /// list and the column table ([`Sampler::start_line`]); the line's first direction
    /// (float(column × dx + x0), float(line × dy + y0), z); per sample column: the pixel
    /// list ([`Sampler::start_column`]), then the background if it is empty, else the body of
    /// `SI_Sample` (with a chunk mark of its own, relinking when slices joined at this
    /// column), with a tag bit per layer shaded at pixel column / level; the direction's x
    /// grows by dx (rounded to float each time). The line then goes to `blur`, or to
    /// `SI_Output` at level 1, or to `antialias`; with tags, every level-th line the tag line
    /// is written and cleared; the chunk mark is released.
    ///
    /// After the last line, unless cropping: the lines below the region as background, and
    /// then **one** depth line of −100000 and **one** tag line, however many lines there
    /// were (`SI_supersample` writes one per line). The depth file is closed.
    ///
    /// Not checked in the emulator as a whole (the licence call at its entry stops the
    /// harness); its parts `SI_Sample`, `antialias`, `blur` and `SI_Output` are. Class:
    /// model.
    pub fn sample_frame(&mut self, env: &mut dyn FrameEnv) {
        let mut lp = self.sample_begin(env);
        let mut y = self.region[3] as i16;
        while y as i32 >= self.region[2] {
            self.line_checks(env);
            self.sample_line(env, &mut lp, y);
            self.sample_line_out(env, y);
            y = y.wrapping_sub(1);
        }
        self.sample_end(env);
    }

    /// `SI_sample`'s set-up, up to its first line (see [`Sampler::sample_frame`]): the
    /// status line, the crop values, the three list heads, the 100 layer rays, the pixel rows
    /// of `antialias`, the depth of field rows, the depth file and the lines above the region.
    /// Returns what the loop carries from line to line besides the sampler.
    pub fn sample_begin(&mut self, env: &mut dyn FrameEnv) -> LineLoop {
        let level = self.level as i32;
        self.begin_message(env);
        self.set_crop();
        env.chunk_malloc(0x28);
        env.chunk_malloc(0x28);
        env.chunk_malloc(0x28);
        let rays: Vec<Ray> = (0..100).map(|i| Ray { w3e: i + 1, ..Ray::default() }).collect();
        if self.level > 1 {
            env.chunk_malloc(self.buffer_len.wrapping_mul(0x18));
            env.chunk_malloc(self.buffer_len.wrapping_mul(0x18));
            // the original leaves each row's last entry as allocated: here blank as well
            let len = self.buffer_len.max(0) as usize;
            self.aa.top = vec![self.blank; len];
            self.aa.bot = vec![self.blank; len];
            self.aa.fc3 = 0;
            self.aa.fc4 = -level;
        }
        let (mut xstart, mut yline) = (self.region[0] as i16, self.region[3] as i16);
        if self.dof_on != 0 {
            env.chunk_malloc(self.dof.lines.wrapping_shl(2));
            self.dof.rows.clear();
            let mut k: i16 = 0;
            while (k as i32) < self.dof.lines {
                env.chunk_malloc(self.region[1].wrapping_add(10).wrapping_mul(0x18));
                // entries from x max on are as allocated in the original: here zero
                let mut row = vec![Rgbazo::default(); (self.region[1] + 10).max(0) as usize];
                let mut i: i16 = 0;
                while (i as i32) < self.region[1] {
                    row[i as usize] = self.blank;
                    i = i.wrapping_add(1);
                }
                self.dof.rows.push(row);
                k = k.wrapping_add(1);
            }
            self.dof.n_call = 0;
            xstart = xstart.wrapping_sub(self.dof.half as i16);
            yline = yline.wrapping_sub(self.dof.half as i16);
        }
        if self.out.depth_file {
            self.depth_begin(env);
        }
        self.lines_above(env);
        self.out.y = self.region_y_px[1] as i16;
        LineLoop { active: None, xstart, yline, rays, ..LineLoop::default() }
    }

    /// The two calls at the head of every line of both loops: the custom effects' abort
    /// check, and `checkForExit` unless the program is the standalone renderer.
    pub fn line_checks(&mut self, env: &mut dyn FrameEnv) {
        env.check_render_abort();
        if self.standalone == 0 {
            env.check_for_exit();
        }
    }

    /// The samples of line `y` of `SI_sample` (see [`Sampler::sample_frame`]), into
    /// [`Sampler::line`]: a chunk mark; the active list and the column table; per sample
    /// column the pixel list, then the background or the shaded layers (with their tag
    /// bits); the mark given back. The original gives the line's mark back after the line's
    /// output: nothing there uses the chunk memory, so it is given back here.
    pub fn sample_line(&mut self, env: &mut dyn SampleEnv, lp: &mut LineLoop, y: i16) {
        let level = self.level as i32;
        let mut line_mark = 0;
        env.chunk().save_current(Some(&mut line_mark));
        self.ss.y = y;
        self.start_line(&mut lp.active, y);
        let mut dir = Vector::new((x(lp.xstart as i32) * self.ss.dir[3] + self.ss.dir[0]).to_f32(), (x(lp.yline as i32) * self.ss.dir[4] + self.ss.dir[1]).to_f32(), self.ss.dir[2]);
        let mut pixel: Option<usize> = None;
        let mut xs = self.region[0] as i16;
        while xs as i32 <= self.region[1] {
            self.x = xs;
            self.start_column(&mut pixel, xs);
            let column = xs as i32 as usize;
            if pixel.is_none() {
                self.line[column] = self.ss.background;
            } else {
                let mut mark = 0;
                env.chunk().save_current(Some(&mut mark));
                let relink = self.columns[column].is_some();
                let layers = {
                    let (tns, verts) = env.geometry();
                    sample_layers(&mut self.slices, &mut pixel, tns, verts, dir, relink, &mut lp.rays)
                };
                let mut colour = self.ss.background;
                for i in (0..layers).rev() {
                    colour = env.shade(&mut lp.rays[i], colour, 1);
                    if self.tags_on != 0 {
                        // a level of 0 is a division by zero in the original as well
                        self.tag_pixel(env, lp.rays[i].tn.expect("a layer has its triangle"), xs as i32 / level);
                    }
                }
                self.line[column] = colour;
                env.chunk().reclaim(mark);
            }
            dir.x = (x(dir.x) + self.ss.dir[3]).to_f32();
            xs = xs.wrapping_add(1);
        }
        env.chunk().reclaim(line_mark);
        lp.yline = lp.yline.wrapping_sub(1);
    }

    /// The output of line `y` of `SI_sample`: the line of samples to `blur`, or to
    /// `SI_Output` at level 1, or to `antialias`; with tags, every level-th line the tag line
    /// is written and cleared.
    pub fn sample_line_out(&mut self, env: &mut dyn FrameEnv, y: i16) {
        let level = self.level as i32;
        if self.dof_on != 0 {
            self.blur(env);
        } else if self.level == 1 {
            output(&mut self.out, env, &self.line, self.sr_x_offset, self.sr_y_offset, self.sr_x_res as i32);
        } else {
            self.antialias(env);
        }
        if self.tags_on != 0 && y as i32 % level == 0 {
            self.tag_line(env);
        }
    }

    /// What `SI_sample` does after its last line (see [`Sampler::sample_frame`]).
    pub fn sample_end(&mut self, env: &mut dyn FrameEnv) {
        if self.crop == 0 {
            let mut py = self.region_y_px[0] as i16;
            loop {
                py = py.wrapping_sub(1);
                if x(py as i32).lt(x(self.picture_y[0])) || x(py as i32).unordered(x(self.picture_y[0])) {
                    break;
                }
                self.background_line(env, py);
            }
            if self.out.depth_file {
                let w = self.out.width.max(0) as usize;
                self.out.depth[..w].fill(NONE);
                env.depth_write(&self.out.depth[..w]);
            }
            if self.tags_on != 0 {
                env.tag_write_scanline(&self.tags);
            }
        }
        if self.out.depth_file {
            env.depth_close();
        }
    }

    /// Renders the frame by adaptive supersampling [soft 0x0825cd98 SI_supersample]:
    /// `SI_supersample()`.
    ///
    /// The original first calls `getRenderLicense` ([`crate::licence::get_render_license`])
    /// and returns at once when that gives 0; on the image that call is made by the loop's
    /// dispatch (`adapter`), and this is the loop after a granted licence.
    ///
    /// Differences from `SI_sample`: one grid cell per pixel (the level is 1 here); a sample
    /// cache of `sub + 1` rows of (x max + 1) × sub + 1 entries, red −1 from entry
    /// sub × x min on; the 100 rays are the file-scope `winner`; with depth of field the
    /// rows are x max samples long, **the first ray direction in the globals is moved back
    /// by `half` steps in x and y** (float(x0 − half × dx)), and `dofSubdivid` is used
    /// instead of `colSubdivid`; one chunk mark for the whole frame. Per pixel: `resort` =
    /// whether slices joined the list here; the value of (x, x + 1, y, y + 1) from the
    /// subdivision routine. With tags, if the first of `winner` holds a triangle, the
    /// models of `winner`'s layers get their bit at pixel x: **these are the layers of the
    /// last sample the subdivision took, which is a neighbouring pixel's when all four
    /// corners came from the cache.** Each line goes to `blur` or `SI_Output`, a tag line
    /// is written every line, and the cache rows turn: row `sub` takes row 0's samples, the
    /// others are emptied. Below the region every line gets its depth line and its tag line.
    ///
    /// Not checked in the emulator as a whole (the licence call at its entry stops the
    /// harness); `colSubdivid`, `dofSubdivid`, `blur` and `SI_Output` are. Class: model.
    pub fn supersample_frame(&mut self, env: &mut dyn FrameEnv) {
        let mut lp = self.supersample_begin(env);
        let mut y = self.region[3] as i16;
        while y as i32 >= self.region[2] {
            self.line_checks(env);
            self.supersample_line(env, &mut lp, y);
            self.supersample_line_out(env);
            y = y.wrapping_sub(1);
        }
        self.supersample_end(env, &lp);
    }

    /// `SI_supersample`'s set-up, up to its first line (see [`Sampler::supersample_frame`]):
    /// the crop values, the status line, the list heads, the sample cache, the 100 rays, the
    /// depth of field rows and moved first direction, the depth file, the lines above the
    /// region and the frame's chunk mark.
    pub fn supersample_begin(&mut self, env: &mut dyn FrameEnv) -> LineLoop {
        self.set_crop();
        self.begin_message(env);
        env.chunk_malloc(0x28);
        env.chunk_malloc(0x28);
        let sub = self.ss.sub;
        let first = sub.wrapping_mul(self.region[0] as i16);
        let len = (self.region[1].wrapping_add(1).wrapping_mul(sub as i32).wrapping_add(1)) as i16;
        env.chunk_malloc((sub as i32) * 4 + 4);
        self.ss.rows.clear();
        let mut k: i16 = 0;
        while k <= sub {
            env.chunk_malloc(len as i32 * 0x18);
            // entries below `first`, and all but the red of the others, are as allocated in
            // the original: here zero
            let mut row = vec![Rgbazo::default(); len.max(0) as usize];
            let mut i = first;
            while i < len {
                row[i as usize].r = EMPTY as f32;
                i = i.wrapping_add(1);
            }
            self.ss.rows.push(row);
            k = k.wrapping_add(1);
        }
        self.ss.winner = (0..100).map(|i| Ray { w3e: i + 1, ..Ray::default() }).collect();
        if self.dof_on != 0 {
            env.chunk_malloc(self.dof.lines.wrapping_shl(2));
            self.dof.rows.clear();
            let mut k: i16 = 0;
            while (k as i32) < self.dof.lines {
                env.chunk_malloc(self.region[1].wrapping_mul(0x18));
                self.dof.rows.push(vec![self.blank; self.region[1].max(0) as usize]);
                k = k.wrapping_add(1);
            }
            self.dof.n_call = 0;
            let h = x(self.dof.half);
            self.ss.dir[1] = (x(self.ss.dir[1]) - h * self.ss.dir[4]).to_f32();
            self.ss.dir[0] = (x(self.ss.dir[0]) - h * self.ss.dir[3]).to_f32();
        }
        if self.out.depth_file {
            self.depth_begin(env);
        }
        self.lines_above(env);
        let mut frame_mark = 0;
        env.chunk().save_current(Some(&mut frame_mark));
        self.out.y = self.region_y_px[1] as i16;
        LineLoop { active: None, first, len, frame_mark, ..LineLoop::default() }
    }

    /// The samples of line `y` of `SI_supersample` (see [`Sampler::supersample_frame`]), into
    /// [`Sampler::line`], and the turn of the cache rows. The original turns the rows after
    /// the line's output: the output reads none of them, so they turn here.
    pub fn supersample_line(&mut self, env: &mut dyn SampleEnv, lp: &mut LineLoop, y: i16) {
        let sub = self.ss.sub;
        self.ss.y = y;
        self.start_line(&mut lp.active, y);
        let mut pixel: Option<usize> = None;
        let mut xs = self.region[0] as i16;
        while xs as i32 <= self.region[1] {
            self.x = xs;
            self.start_column(&mut pixel, xs);
            let column = xs as i32 as usize;
            self.ss.resort = self.columns[column].is_some() as i16;
            if pixel.is_none() {
                self.line[column] = self.ss.background;
            } else {
                let (x0, x1) = (xs as f32, (x(xs as i32) + F80::ONE).to_f32());
                let (y0, y1) = (y as f32, (x(y as i32) + F80::ONE).to_f32());
                self.line[column] = if self.dof_on != 0 {
                    dof_subdivid(&mut self.ss, env, &mut self.slices, &mut pixel, x0, x1, y0, y1)
                } else {
                    col_subdivid(&mut self.ss, env, &mut self.slices, &mut pixel, x0, x1, y0, y1)
                };
                if self.tags_on != 0 && self.ss.winner[0].tn.is_some() {
                    let mut k = 0;
                    loop {
                        // (with 100 layers the original reads the word after the array)
                        let Some(tn) = self.ss.winner.get(k).and_then(|r| r.tn) else { break };
                        self.tag_pixel(env, tn, xs as i32);
                        k += 1;
                    }
                }
            }
            xs = xs.wrapping_add(1);
        }
        let s = sub as usize;
        self.ss.rows.swap(0, s);
        let mut k: i16 = 0;
        while k < sub {
            let mut i = lp.first;
            while i < lp.len {
                self.ss.rows[k as usize][i as usize].r = EMPTY as f32;
                i = i.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
    }

    /// The output of a line of `SI_supersample`: the line of samples to `blur` or to
    /// `SI_Output`; with tags, the tag line written and cleared.
    pub fn supersample_line_out(&mut self, env: &mut dyn FrameEnv) {
        if self.dof_on != 0 {
            self.blur(env);
        } else {
            output(&mut self.out, env, &self.line, self.sr_x_offset, self.sr_y_offset, self.sr_x_res as i32);
        }
        if self.tags_on != 0 {
            self.tag_line(env);
        }
    }

    /// What `SI_supersample` does after its last line (see [`Sampler::supersample_frame`]).
    pub fn supersample_end(&mut self, env: &mut dyn FrameEnv, lp: &LineLoop) {
        if self.crop == 0 {
            let mut py = self.region_y_px[0] as i16;
            loop {
                py = py.wrapping_sub(1);
                if x(py as i32).lt(x(self.picture_y[0])) || x(py as i32).unordered(x(self.picture_y[0])) {
                    break;
                }
                self.background_line(env, py);
                if self.out.depth_file {
                    let w = self.out.width.max(0) as usize;
                    self.out.depth[..w].fill(NONE);
                    env.depth_write(&self.out.depth[..w]);
                }
                if self.tags_on != 0 {
                    env.tag_write_scanline(&self.tags);
                }
            }
        }
        env.chunk().reclaim(lp.frame_mark);
        if self.out.depth_file {
            env.depth_close();
        }
    }

    /// The active list and column table of line `y`, as [`Sampler::start_line`] keeps them,
    /// without sampling the line: what a band of lines rendered on its own replays for the
    /// lines above it (`crate::bands`). The direction line of `SI_sample` steps with it.
    pub fn skip_line(&mut self, lp: &mut LineLoop, y: i16) {
        self.ss.y = y;
        self.start_line(&mut lp.active, y);
        lp.yline = lp.yline.wrapping_sub(1);
    }
}

/// What the two frame loops carry from line to line besides the [`Sampler`]: locals of
/// `SI_sample` and `SI_supersample`, not a record of the original.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineLoop {
    /// The active slice list.
    pub active: Option<usize>,
    /// `SI_sample`: the first sample column and the line of the next line's first ray.
    pub xstart: i16,
    pub yline: i16,
    /// `SI_sample`: the 100 layer rays (`SI_supersample`'s are `winner`).
    pub rays: Vec<Ray>,
    /// `SI_supersample`: the first entry of a cache row in use (sub × x min) and the rows'
    /// length.
    pub first: i16,
    pub len: i16,
    /// `SI_supersample`: the frame's chunk mark.
    pub frame_mark: i32,
}

/// What `checkForExit` asks of the user interface.
pub trait ExitHost {
    /// The word at 0x086793f8: a tablet is in use.
    fn tablet(&self) -> bool;
    /// `SI_tabletGetButtonUpEvent()`.
    fn tablet_button_up_event(&mut self);
    /// `getbutton(device)`: whether a key or mouse button is down.
    fn getbutton(&mut self, device: i32) -> i32;
    /// The word at 0x086794b4: the last device event seen.
    fn last_device(&self) -> i32;
    /// `SI_voluntaryExit(code, flag)`.
    fn voluntary_exit(&mut self, code: i32, flag: i16);
}

/// Lets the user stop an interactive render [soft 0x0825b800 checkForExit]:
/// `checkForExit()`. With a tablet its pending button event is fetched first. If device
/// 0x66 is down, or was the last device seen, or device 7 is down, `SI_voluntaryExit(4,
/// display off)` is called and 1 returned; else 0. (Devices 0x66 and 7 are by the names of
/// the GL device table the middle mouse button and the escape key: named.) `display` is the
/// switch at 0x08790d58.
pub fn check_for_exit(host: &mut dyn ExitHost, display: bool) -> i32 {
    if host.tablet() {
        host.tablet_button_up_event();
    }
    if host.getbutton(0x66) != 0 || host.last_device() == 0x66 || host.getbutton(7) != 0 {
        host.voluntary_exit(4, !display as i16);
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A triangle in the plane z = `z` with corners (0,0), (4,0), (0,4), as `barr` leaves it.
    fn triangle(verts: &mut Vec<RVertex>, z: f32) -> Tn {
        let base = verts.len();
        for (px, py) in [(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)] {
            verts.push(RVertex { world: Vector::new(px, py, z), ..RVertex::default() });
        }
        Tn {
            v: [Some(base), Some(base + 1), Some(base + 2)],
            n: Vector::new(0.0, 0.0, 16.0),
            d: 16.0 * z,
            inv_d: 1.0 / (16.0 * z),
            axis: 3,
            inv_n: 1.0 / 16.0,
            edge: [Vector::new(-4.0, 4.0, 0.0), Vector::new(0.0, -4.0, 0.0), Vector::new(4.0, 0.0, 0.0)],
            ..Tn::default()
        }
    }

    /// Geometry, a chunk buffer and a stand-in for `SI_shade` that adds 1 to the red.
    struct Env {
        tns: Vec<Tn>,
        verts: Vec<RVertex>,
        order: Vec<(usize, f32, i16)>,
        chunk: ChunkBuffer,
    }
    impl SampleEnv for Env {
        fn geometry(&self) -> (&[Tn], &[RVertex]) {
            (&self.tns, &self.verts)
        }
        fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
            self.order.push((ray.tn.unwrap(), under.r, primary));
            Rgbazo { r: under.r + 1.0, ..under }
        }
        fn chunk(&mut self) -> &mut ChunkBuffer {
            &mut self.chunk
        }
        fn chunk_malloc(&mut self, _: i32) -> u32 {
            0
        }
    }

    #[test]
    fn general_intersection() {
        let mut verts = Vec::new();
        let tns = vec![triangle(&mut verts, -8.0)];
        let mut ray = Ray { origin: Vector::new(1.0, 1.0, 0.0), dir: Vector::new(0.0, 0.0, -1.0), t: 100.0, ..Ray::default() };
        assert!(intersect(&mut ray, &tns, &verts, 0, None, 0.0));
        assert_eq!(ray.t, 8.0);
        // weights: corner 0 gets 16 − 4 − 4 = 8, corners 1 and 2 get 4 each; they sum to n.z
        assert_eq!(ray.hit, Vector::new(8.0, 4.0, 4.0));
        assert_eq!(ray.tn, Some(0));
        // not nearer than what the ray already has; excluded; below tmin; outside; parallel
        assert!(!intersect(&mut ray, &tns, &verts, 0, None, 0.0));
        let fresh = |o: Vector, d: Vector| Ray { origin: o, dir: d, t: 100.0, ..Ray::default() };
        let down = Vector::new(0.0, 0.0, -1.0);
        assert!(!intersect(&mut fresh(Vector::new(1.0, 1.0, 0.0), down), &tns, &verts, 0, Some(0), 0.0));
        assert!(!intersect(&mut fresh(Vector::new(1.0, 1.0, 0.0), down), &tns, &verts, 0, None, 8.5));
        assert!(!intersect(&mut fresh(Vector::new(3.0, 3.0, 0.0), down), &tns, &verts, 0, None, 0.0));
        assert!(!intersect(&mut fresh(Vector::new(1.0, 1.0, 0.0), Vector::new(1.0, 0.0, 0.0)), &tns, &verts, 0, None, 0.0));
        // on an edge and on a corner: inside
        assert!(intersect(&mut fresh(Vector::new(2.0, 2.0, 0.0), down), &tns, &verts, 0, None, 0.0));
        assert!(intersect(&mut fresh(Vector::new(0.0, 0.0, 0.0), down), &tns, &verts, 0, None, 0.0));
        // from behind the plane the test is the same (two-sided)
        assert!(intersect(&mut fresh(Vector::new(1.0, 1.0, -16.0), Vector::new(0.0, 0.0, 1.0)), &tns, &verts, 0, None, 0.0));
    }

    /// Random and borderline triangles and rays for [`certain_miss_is_a_miss`]: corners on a
    /// grid (exact sums, points exactly on edges and corners) or anywhere, at scales from
    /// 2⁻¹² to 2¹², directions aimed at barycentric points on and near the edges or nearly in
    /// the plane (|d · n| near 1e−8), distances and `tmin` at the hit's float and its
    /// neighbours, and now and then a non-finite or arbitrary member.
    fn miss_case(r: &mut si_x87::x87::testing::Rng, verts: &mut Vec<RVertex>) -> (Tn, Ray, f32) {
        let mut unit = || (r.next() >> 11) as f64 / (1u64 << 53) as f64;
        let mut vals = Vec::with_capacity(64);
        for _ in 0..64 {
            vals.push(unit());
        }
        let mut it = vals.into_iter();
        let mut u = move || it.next().unwrap();
        let scale = 2f32.powi((u() * 24.0) as i32 - 12);
        let grid = u() < 0.4;
        let coord = |u: &mut dyn FnMut() -> f64| {
            let c = (u() * 2.0 - 1.0) as f32 * 8.0;
            if grid { c.round() * scale } else { c * scale }
        };
        let pt = |u: &mut dyn FnMut() -> f64| Vector::new(coord(u), coord(u), coord(u));
        let (v0, v1, v2) = (pt(&mut u), pt(&mut u), pt(&mut u));
        let sub = |a: Vector, b: Vector| Vector::new(a.x - b.x, a.y - b.y, a.z - b.z);
        let (a, b) = (sub(v1, v0), sub(v2, v0));
        let mut n = Vector::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
        let mut dd = n.x * v0.x + n.y * v0.y + n.z * v0.z;
        let axis = if n.x.abs() >= n.y.abs() && n.x.abs() >= n.z.abs() { 1 } else if n.y.abs() >= n.z.abs() { 2 } else { 3 };
        let base = verts.len();
        for v in [v0, v1, v2] {
            verts.push(RVertex { world: v, ..RVertex::default() });
        }
        let mut edge = [sub(v2, v1), sub(v0, v2), sub(v1, v0)];
        let special = [f32::NAN, f32::INFINITY, -f32::INFINITY, 0.0, -0.0, 1e-40, f32::MAX];
        if u() < 0.03 {
            let s = special[(u() * 7.0) as usize % 7];
            match (u() * 4.0) as usize {
                0 => n.x = s,
                1 => dd = s,
                2 => edge[1].y = s,
                _ => n.z = s,
            }
        }
        if u() < 0.03 {
            dd = (u() * 2.0 - 1.0) as f32 * 100.0 * scale;
        }
        let tn = Tn { v: [Some(base), Some(base + 1), Some(base + 2)], n, d: dd, axis, edge, ..Tn::default() };
        // a barycentric point: on an edge, a corner, just outside or anywhere
        let bary = |u: &mut dyn FnMut() -> f64| match (u() * 6.0) as usize {
            0 => 0.0f32,
            1 => 1.0,
            2 => (u() * 1e-6) as f32 - 5e-7,
            3 => 1.0 + (u() * 1e-6) as f32 - 5e-7,
            _ => (u() * 1.4 - 0.2) as f32,
        };
        let (ba, bb) = (bary(&mut u), bary(&mut u));
        let bc = 1.0 - ba - bb;
        let target = Vector::new(v0.x * ba + v1.x * bb + v2.x * bc, v0.y * ba + v1.y * bb + v2.y * bc, v0.z * ba + v1.z * bb + v2.z * bc);
        let origin = if grid && u() < 0.5 { Vector::new(coord(&mut u) * 3.0, coord(&mut u) * 3.0, coord(&mut u) * 3.0) } else { pt(&mut u) };
        let mut dir = sub(target, origin);
        if u() < 0.15 {
            // nearly in the plane: an edge plus a little of the normal, d · n about ±1e−8
            let nn = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
            let e = edge[(u() * 3.0) as usize % 3];
            let el = (e.x * e.x + e.y * e.y + e.z * e.z).sqrt();
            let k = (1e-8 * (1.0 + (u() * 4.0 - 2.0) * 1e-6)) as f32 / (nn * nn) * if u() < 0.5 { 1.0 } else { -1.0 };
            dir = Vector::new(e.x / el + k * n.x, e.y / el + k * n.y, e.z / el + k * n.z);
        } else if u() < 0.5 {
            let l = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
            dir = Vector::new(dir.x / l, dir.y / l, dir.z / l);
        }
        // the distance and tmin at the hit's float, its neighbours, or far
        let dn = dir.x as f64 * n.x as f64 + dir.y as f64 * n.y as f64 + dir.z as f64 * n.z as f64;
        let t = ((dd as f64 - (origin.x as f64 * n.x as f64 + origin.y as f64 * n.y as f64 + origin.z as f64 * n.z as f64)) / dn) as f32;
        let near = |u: &mut dyn FnMut() -> f64| f32::from_bits(t.to_bits().wrapping_add((u() * 5.0) as u32).wrapping_sub(2));
        let rt = match (u() * 4.0) as usize {
            0 => NO_HIT,
            1 => f32::INFINITY,
            _ => near(&mut u),
        };
        let tmin = match (u() * 4.0) as usize {
            0 => near(&mut u),
            1 => 1e-4,
            _ => 0.0,
        };
        (tn, Ray { origin, dir, t: rt, ..Ray::default() }, tmin)
    }

    /// [`certain_miss`] says "miss" only where the exact test misses, on a million cases of
    /// [`miss_case`]; bounds made smaller than zero must be caught; and it decides most misses.
    #[test]
    fn certain_miss_is_a_miss() {
        let mut r = si_x87::x87::testing::Rng(0x5eed_4444);
        let n = 1_000_000;
        let (mut hits, mut misses, mut decided, mut wrong, mut wrong_mutant) = (0u64, 0u64, 0u64, 0u64, 0u64);
        for _ in 0..n {
            let mut verts = Vec::new();
            let (tn, ray, tmin) = miss_case(&mut r, &mut verts);
            let hit = intersect_exact(&mut ray.clone(), &tn, &verts, 0, tmin);
            let cm = certain_miss_k(&ray, &tn, &verts, tmin, 1.0);
            if hit {
                hits += 1;
                if cm {
                    wrong += 1;
                    eprintln!("certain_miss on a hit: {tn:?} {ray:?} {tmin:e}");
                }
            } else {
                misses += 1;
                decided += cm as u64;
            }
            wrong_mutant += (hit && certain_miss_k(&ray, &tn, &verts, tmin, -1.0)) as u64;
        }
        eprintln!("certain_miss: {n} cases, {hits} hits, {misses} misses of which {decided} decided, {wrong} wrong; bounds of −1×: {wrong_mutant} wrong");
        assert_eq!(wrong, 0);
        assert!(wrong_mutant > 0, "the test cannot tell bounds below zero");
        assert!(hits > n / 20 && decided > misses / 2, "{hits} {misses} {decided}");
    }

    #[test]
    fn negative_normal_reverses_the_limits() {
        let mut verts = Vec::new();
        let mut tn = triangle(&mut verts, -8.0);
        // the same triangle wound the other way: normal and edges negated
        tn.n = Vector::new(0.0, 0.0, -16.0);
        tn.d = 128.0;
        tn.v.swap(1, 2);
        tn.edge = [Vector::new(4.0, -4.0, 0.0), Vector::new(-4.0, 0.0, 0.0), Vector::new(0.0, 4.0, 0.0)];
        let tns = vec![tn];
        let mut ray = Ray { origin: Vector::new(1.0, 1.0, 0.0), dir: Vector::new(0.0, 0.0, -1.0), t: 100.0, ..Ray::default() };
        assert!(intersect(&mut ray, &tns, &verts, 0, None, 0.0));
        assert_eq!(ray.hit, Vector::new(-8.0, -4.0, -4.0));
        ray = Ray { origin: Vector::new(3.0, 3.0, 0.0), dir: Vector::new(0.0, 0.0, -1.0), t: 100.0, ..Ray::default() };
        assert!(!intersect(&mut ray, &tns, &verts, 0, None, 0.0));
    }

    #[test]
    fn eye_ray_tests_use_the_key() {
        let mut verts = Vec::new();
        let tns = vec![triangle(&mut verts, -8.0)];
        // direction (1, 1, −8) / 8 reaches (1, 1, −8) at t = 8: key 1/8
        let dir = Vector::new(0.125, 0.125, -1.0);
        let mut ray = Ray { dir, ..Ray::default() };
        let s = Slice { key: 0.125, tn: 0, ..Slice::default() };
        assert_eq!(intersect1(&mut ray, &s, &tns, &verts, 0.0), (1, 0));
        assert_eq!((ray.t, ray.hit, ray.tn), (8.0, Vector::new(8.0, 4.0, 4.0), Some(0)));
        // key 0: a miss; a negative key is tried by the first routine and refused by the second
        let mut ray = Ray { dir, ..Ray::default() };
        assert_eq!(intersect1(&mut ray, &Slice { key: 0.0, ..s }, &tns, &verts, 0.0), (2, 0));
        assert_eq!(intersect2(&mut ray, &Slice { key: -0.125, ..s }, &tns, &verts), 2);
        assert_eq!(intersect2(&mut ray, &s, &tns, &verts), 0);
        assert_eq!((ray.t, ray.tn, ray.hit), (8.0, Some(0), Vector::default()));
        // a wrong key puts the point elsewhere: outside
        assert_eq!(intersect2(&mut ray, &Slice { key: 0.01, ..s }, &tns, &verts), 2);
    }

    #[test]
    fn sample_orders_and_stacks_layers() {
        let mut verts = Vec::new();
        // far (z = −16, opaque), near (z = −8, lets the background through), and one beside
        let mut tns = vec![triangle(&mut verts, -16.0), triangle(&mut verts, -8.0), triangle(&mut verts, -4.0)];
        tns[1].flags2 = 1;
        for v in verts[6..9].iter_mut() {
            v.world.x += 50.0;
        }
        let mut slices: Vec<Slice> = (0..3).map(|i| Slice { tn: i as u32, link: if i < 2 { Some(i + 1) } else { None }.into(), ..Slice::default() }).collect();
        let mut head = Some(0);
        let mut rays = vec![Ray::default(); 100];
        let bg = Rgbazo { r: 0.5, ..Rgbazo::default() };
        let mut env = Env { tns, verts, order: Vec::new(), chunk: ChunkBuffer::default() };
        let out = sample(&mut env, &mut slices, &mut head, Vector::new(1.0, 1.0, -16.0), true, &mut rays, bg);
        // keys: 1/t for the unit direction; nearest first: triangle 2 (missed), 1, 0
        assert!(slices[2].key > slices[1].key && slices[1].key > slices[0].key);
        assert_eq!((slices[2].by_depth.get(), slices[1].by_depth.get(), slices[0].by_depth.get()), (Some(1), Some(0), None));
        // shaded back to front: the far one over the background, then the near one
        assert_eq!(env.order, vec![(0, 0.5, 1), (1, 1.5, 1)]);
        assert_eq!(out.r, 2.5);
        // the pixel's own list is now farthest first
        assert_eq!(head, Some(0));
        assert_eq!((slices[0].link.get(), slices[1].link.get(), slices[2].link.get()), (Some(1), Some(2), None));
        // the unit direction went into the ray
        let len = (rays[0].dir.x * rays[0].dir.x + rays[0].dir.y * rays[0].dir.y + rays[0].dir.z * rays[0].dir.z).sqrt();
        assert!((len - 1.0).abs() < 1e-6);
        // the chunk mark was taken and given back: one mark saved (null: nothing allocated
        // yet, so the release fails and the mark stays counted)
        assert_eq!(env.chunk.marks, 1);
        // nothing hit: the background, untouched
        let mut head = Some(2);
        slices[2].link = SliceRef::NONE;
        env.order.clear();
        let out = sample(&mut env, &mut slices, &mut head, Vector::new(1.0, 1.0, -16.0), false, &mut rays, bg);
        assert_eq!(out, bg);
        assert!(env.order.is_empty());
    }

    /// The frame loop's surroundings: the scene of [`Env`] and a record of the lines sent
    /// to the picture file.
    struct Frame {
        env: Env,
        lines: Vec<(i32, Vec<u8>)>,
    }
    impl SampleEnv for Frame {
        fn geometry(&self) -> (&[Tn], &[RVertex]) {
            self.env.geometry()
        }
        fn shade(&mut self, _: &mut Ray, under: Rgbazo, _: i16) -> Rgbazo {
            Rgbazo { r: under.r + 0.5, ..under }
        }
        fn chunk(&mut self) -> &mut ChunkBuffer {
            &mut self.env.chunk
        }
        fn chunk_malloc(&mut self, _: i32) -> u32 {
            0
        }
    }
    impl LineSink for Frame {
        fn dither_scan(&mut self, _: &[Rgbazo], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [f32]) {}
        fn incrustation(&mut self, _: &mut crate::renutil::OutputLine, _: i32) {}
        fn rle_write(&mut self, y: i32, width: i32, line: &crate::renutil::OutputLine) {
            self.lines.push((y, line.red[..width as usize].to_vec()));
        }
        fn depth_write(&mut self, _: &[f32]) {}
        fn fb_write(&mut self, _: i16) {}
        fn status(&mut self, _: &str) {}
    }
    impl FrameEnv for Frame {
        fn display_status(&mut self, _: &str) {}
        fn check_render_abort(&mut self) {}
        fn check_for_exit(&mut self) {}
        fn depth_open(&mut self, _: &[u8]) {}
        fn depth_close(&mut self) {}
        fn tag_write_scanline(&mut self, _: &TagImage) {}
    }

    /// A frame of 4 × 3 samples at level 1 with one slice on lines 2 and 1, columns 1 to 3.
    fn small_frame() -> (Sampler, Frame) {
        let mut verts = Vec::new();
        let tns = vec![triangle(&mut verts, -8.0)];
        let bytes = || vec![0u8; 14];
        let s = Sampler {
            level: 1,
            region: [0, 3, 0, 2],
            region_y_px: [0, 2],
            picture_y: [0.0, 2.0],
            window_x: [0.0, 3.0],
            // sample (x, y) looks at the point (x + 0.5, y + 0.5) of the plane z = −8
            ss: Supersample { sub: 1, dir: [0.0625, 0.0625, -1.0, 0.125, 0.125], background: Rgbazo { r: 0.25, ..Rgbazo::default() }, contrast: 10.0, ..Supersample::default() },
            scan_lines: vec![None, None, Some(0)],
            columns: vec![None; 6],
            slices: vec![Slice { x0: 1.0, x1: 3.0, y_end: 1, tn: 0, ..Slice::default() }],
            line: vec![Rgbazo::default(); 6],
            out: OutputState { x0: 0, x1: 3, red: bytes(), green: bytes(), blue: bytes(), alpha: bytes(), depth: vec![0.0; 14], line: crate::renutil::OutputLine { red: bytes(), green: bytes(), blue: bytes(), alpha: bytes() }, width: 4, to_file: true, ..OutputState::default() },
            ..Sampler::default()
        };
        (s, Frame { env: Env { tns, verts, order: Vec::new(), chunk: ChunkBuffer::default() }, lines: Vec::new() })
    }

    #[test]
    fn regular_frame_loop() {
        let (mut s, mut f) = small_frame();
        s.sample_frame(&mut f);
        // background 0.25 → 63; a shaded sample 0.75 → 191. The triangle x + y ≤ 4 holds the
        // sample points (1.5, 2.5), (1.5, 1.5) and (2.5, 1.5); line 0 has no slice any more
        assert_eq!(f.lines, vec![(2, vec![63, 191, 63, 63]), (1, vec![63, 191, 191, 63]), (0, vec![63, 63, 63, 63])]);
        assert_eq!((s.out.y, s.ss.y, s.x), (-1, 0, 3));
        // the slice's key is 1 / distance of the last sample that tested it
        assert!(s.slices[0].key > 0.0);
    }

    #[test]
    fn adaptive_frame_loop() {
        let (mut s, mut f) = small_frame();
        s.supersample_frame(&mut f);
        // a pixel is the mean of its four corner samples; (1, 2) has the corners (1, 2),
        // (2, 2), (1, 3), (2, 3) of which only (1, 2) lies in the triangle: 0.25 + 0.5 / 4
        let px = |hits: f32| super::super::renutil::channel_byte(0.25 + 0.5 * hits / 4.0);
        assert_eq!(f.lines.len(), 3);
        assert_eq!(f.lines[0], (2, vec![63, px(1.0), 63, 63]));
        // (1, 1): corners (1, 1), (2, 1), (1, 2) inside; (2, 1): (2, 1) only; (3, 1): none
        assert_eq!(f.lines[1], (1, vec![63, px(3.0), px(1.0), 63]));
        assert_eq!(f.lines[2], (0, vec![63, 63, 63, 63]));
        // the cache turned after each line: row `sub` holds what was the line's lower edge
        assert_eq!(s.ss.rows.len(), 2);
    }

    #[test]
    fn filter_table_matches_the_dump() {
        // level 2 and the corners of levels 3 and 5 as dumped from 0x0869cf80
        let f = filter_for_level(2);
        assert_eq!(&f[0][..3], &[0.0625, 0.125, 0.0625]);
        assert_eq!(&f[1][..3], &[0.125, 0.25, 0.125]);
        assert_eq!(f[0][3], 0.0);
        assert_eq!(filter_for_level(3)[2][2], 1.0 / 9.0);
        assert_eq!(filter_for_level(3)[0][0], (1.0f64 / 81.0) as f32);
        assert_eq!(filter_for_level(5)[4][4], 0.04);
        assert_eq!(filter_for_level(4)[3][0], 0.015625);
        assert_eq!(filter_for_level(1)[0][0], 1.0);
    }

    #[test]
    fn antialias_level_two() {
        // level 2: filter 3 wide; sample columns 0..=7 make pixels 0..3 (pixel p is centred
        // on sample 2p + 1); the first line counts for the top row only (row 0 of the filter)
        let blank = Rgbazo { z: -100000.0, o: -100000.0, ..Rgbazo::default() };
        let mut aa = Antialias::new(2, 14, blank);
        assert_eq!((aa.size, aa.fc3, aa.fc4), (3, 0, -2));
        let white = Rgbazo { r: 1.0, g: 1.0, b: 1.0, a: 1.0, z: -5.0, o: 0.0 };
        let line = vec![white; 8];
        let mut rows = Vec::new();
        for _ in 0..3 {
            aa.antialias(&line, 0, 7, 0, &mut |row| rows.push(row.to_vec()));
        }
        // after three lines the top row is complete: weights sum to 1 where the window fits
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0].r, 1.0);
        assert_eq!(rows[0][0].z, -5.0);
        assert_eq!(rows[0][2].a, 1.0);
        // pixel 1's window starts at sample 2 (the second counter starts at −2)
        assert_eq!(rows[0][1].g, 1.0);
        // the top row was reset, the bottom row has had one line (its row 0: a quarter)
        assert_eq!(aa.top[0], blank);
        assert_eq!((aa.fc3, aa.fc4), (-1, 1));
        assert_eq!(aa.bot[0].r, 0.25);
        // the sixth value is never accumulated
        assert_eq!(rows[0][0].o, -100000.0);
    }
}
