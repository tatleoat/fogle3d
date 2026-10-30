// SPDX-License-Identifier: MIT
//! NURBS curves and surfaces: the records, their allocation and copying, and evaluation
//! (finding 0025).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`: 23 of the 33 functions of the original's `eval.c` (span search,
//! evaluation, the normal, knot insertion, sampling), 21 of the 33 of `nrb_b.c` (records,
//! copies, disposal, the exchange of U and V, curve reversal), 10 of the 22 of `nrb_misc.c`
//! and 4 of the 8 of `nrb_geometry.c` (the length tables built, the length and the parameter
//! at a distance, [`geometry`]). Every one is compared with the emulated original (finding
//! 0011; `nurbs-emucheck`).
//!
//! The data model is `si-types`' records of the original's 32-bit layout
//! ([`NurbsDir`], 0x24 bytes; [`NurbsCrv`], 100; [`NurbsSrf`], 0xdc; [`NurbsPoint`], 0x24)
//! and [`own::CompCrv`] (0x18), which this crate lays out. **Only [`own`] holds what a
//! record points to**: a [`own::Curve`] is a curve record with its knots and points beside
//! it, a [`own::Surface`] a surface record with its two knot vectors, its points and its
//! trim lists. The numerical modules see records and slices only, through the three views
//! [`Dir`], [`CrvRef`] and [`SrfRef`].
//!
//! How the original evaluates, in short (all read from code, finding 0025):
//!
//! - a direction of order `k` with `n` points has **`n + k − 2` knots** when open (the
//!   parameter range is `knots[k−2] ..= knots[n−1]`) and `n + 1` when closed (the range is
//!   the whole vector, one period; the knots repeat with that period);
//! - the span is found by a bisection that starts from where the last call ended
//!   ([`knots::interv`], a static of the original: [`Statics::left`]); the search clamps its
//!   copy of the parameter to the range, the evaluation that follows does not;
//! - a point is de Boor's triangle on the `k` control points of the span, in homogeneous
//!   coordinates (the stored x, y, z are already multiplied by the weight), every level
//!   rounded to `double`; a blend factor within 1e-6 of 0 or 1, **or outside that
//!   interval**, copies a point instead of blending; the division by the weight comes last;
//! - a surface point is the curve point of a one-span curve extracted across the surface
//!   ([`eval::srf_extract_curve_seg`]); the normal is the cross product of the two first
//!   derivatives, with a search along the parameters where one vanishes
//!   ([`eval::srf_value_and_normal`]);
//! - a knot is inserted by Boehm's method ([`insert`]); the parameters at which a direction
//!   is sampled for display and conversion are in [`sample`].
//!
//! Arithmetic follows the original's x87 instruction sequences on [`si_x87::F80`]: the same
//! operations in the same order, rounded to `double` exactly where the original stores.

pub mod convert;
pub mod edit;
pub mod eval;
pub mod geometry;
pub mod image;
pub mod insert;
pub mod knots;
pub mod misc;
pub mod own;
pub mod sample;
pub mod trimline;
pub mod trimutils;
pub mod fit;
pub mod fit2;
pub mod trimmed;
pub mod ray_trace;
pub mod blend;
pub mod blend2;
pub mod bridge;
pub mod composite;
pub mod geom3;
pub mod geom5;
// (`trim` includes si-gen's list.rs by path and uses `inter`: not in a test build, see `inter` below)
#[cfg(not(test))]
pub mod trim;
#[cfg(test)]
mod tests;

pub use si_types::geom::{NurbsCrv, NurbsDir, NurbsPoint, NurbsSrf};
use si_types::Ptr32;
use si_x87::F80;

/// The tolerance of every blend-factor and weight test of `eval.c`: the double nearest
/// 1e-6. The compiler emitted one copy per function ((0x08661660) and following; the
/// finding lists which address each routine reads); all hold the bits 0x3eb0c6f7a0b5ed8d.
pub const EPS: f64 = 1e-6;

/// The tolerance of the span search (0x08661700): the double nearest 1e-5.
pub const KNOT_EPS: f64 = 1e-5;

/// The statics of `eval.c` that outlive a call and can change a result.
///
/// The span search keeps the index it last returned ((0x086d9c20, `left.272`), 0 when the
/// program starts) and begins its bisection there when the index lies in the range of the
/// direction it is given. The answer does not depend on the start for a parameter inside a
/// span, but the search does not end at all for some malformed knot vectors from one start
/// and ends from another, so the value is carried.
///
/// The scratch arrays of the segment extraction ((0x086d9c0c, `aj.249`),
/// (0x086d9c10, `dl.250`), (0x086d9c14, `dr.251`) with their sizes at 0x086d9c1c and
/// 0x086d9c18) are grown and never freed; every element is written before it is read, so
/// the port uses locals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statics {
    pub left: i32,
}

/// One parametric direction as the numerical routines see it: the record and its knots.
#[derive(Clone, Copy, Debug)]
pub struct Dir<'a> {
    pub rec: &'a NurbsDir,
    pub knots: &'a [f64],
}

/// The words beside a knot array in the memory image, which the original reads when an extraction walks off the array (the
/// heap's chunk header before it, the next block after it): registered by the image forms for the length of one call
/// ([`MarginGuard`]), keyed by the address of the Rust copy of the array.
struct Margin {
    ptr: usize,
    len: usize,
    before: Vec<f64>,
    after: Vec<f64>,
}

thread_local! {
    static MARGINS: std::cell::RefCell<Vec<Margin>> = const { std::cell::RefCell::new(Vec::new()) };
    static MARGIN_READS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Registered margins of some knot arrays, removed when dropped.
pub struct MarginGuard(usize);

impl MarginGuard {
    /// `arrays`: for each knot array, the Rust copy and the words before (nearest first) and after it in the image.
    pub fn new(arrays: Vec<(&[f64], Vec<f64>, Vec<f64>)>) -> MarginGuard {
        MARGINS.with(|m| {
            let mut m = m.borrow_mut();
            let n = m.len();
            for (k, before, after) in arrays {
                if !k.is_empty() {
                    m.push(Margin { ptr: k.as_ptr() as usize, len: k.len(), before, after });
                }
            }
            MarginGuard(n)
        })
    }
}

impl Drop for MarginGuard {
    fn drop(&mut self) {
        MARGINS.with(|m| m.borrow_mut().truncate(self.0));
    }
}

/// How many knots beside an array were read from the image since the last call (what they hold in the original is a heap
/// header or the neighbouring block; a check cannot hold the port to them).
pub fn take_margin_reads() -> u32 {
    MARGIN_READS.with(|c| c.replace(0))
}

impl<'a> Dir<'a> {
    /// Knot `i` as loaded into the unit, or `None` where the original would read outside
    /// the array (its result is then whatever the heap holds) and the image form has not
    /// registered the words beside the array ([`MarginGuard`]).
    pub fn k(&self, i: i32) -> Option<F80> {
        if let Some(v) = usize::try_from(i).ok().and_then(|i| self.knots.get(i)) {
            return Some(F80::from_f64(*v));
        }
        if self.knots.is_empty() {
            return None;
        }
        let p = self.knots.as_ptr() as usize;
        MARGINS.with(|m| {
            let m = m.borrow();
            let g = m.iter().rev().find(|g| g.ptr == p && g.len == self.knots.len())?;
            let v = if i < 0 { g.before.get((-(i as i64) - 1) as usize) } else { g.after.get(i as usize - g.len) }?;
            MARGIN_READS.with(|c| c.set(c.get() + 1));
            Some(F80::from_f64(*v))
        })
    }
}

/// A curve as the numerical routines see it.
#[derive(Clone, Copy, Debug)]
pub struct CrvRef<'a> {
    pub rec: &'a NurbsCrv,
    pub knots: &'a [f64],
    pub points: &'a [NurbsPoint],
}

impl<'a> CrvRef<'a> {
    pub fn dir(&self) -> Dir<'a> {
        Dir { rec: &self.rec.dir, knots: self.knots }
    }
}

/// A surface as the numerical routines see it. Point (i in U, j in V) is at index
/// `j × u.nb_points + i`: **V is the major direction** (read from every routine that
/// indexes the array).
#[derive(Clone, Copy, Debug)]
pub struct SrfRef<'a> {
    pub rec: &'a NurbsSrf,
    pub knots_u: &'a [f64],
    pub knots_v: &'a [f64],
    pub points: &'a [NurbsPoint],
}

impl<'a> SrfRef<'a> {
    pub fn u(&self) -> Dir<'a> {
        Dir { rec: &self.rec.u, knots: self.knots_u }
    }
    pub fn v(&self) -> Dir<'a> {
        Dir { rec: &self.rec.v, knots: self.knots_v }
    }
}

/// A placeholder for a pointer the Rust replaces by an owned array: any non-null value.
pub(crate) fn held<T: ?Sized>() -> Ptr32<T> {
    Ptr32::new(1)
}

/// The four doubles of a control point: x, y, z already multiplied by the weight, and the
/// weight.
pub fn xyzw(p: &NurbsPoint) -> [f64; 4] {
    [p.x.0, p.y.0, p.z.0, p.w.0]
}

/// Sets coordinate `i` (0 x, 1 y, 2 z, 3 w) of a control point.
pub fn set_coord(p: &mut NurbsPoint, i: usize, v: f64) {
    match i {
        0 => p.x.0 = v,
        1 => p.y.0 = v,
        2 => p.z.0 = v,
        _ => p.w.0 = v,
    }
}

/// Coordinate `i` of a control point as loaded into the unit.
pub(crate) fn co(p: &NurbsPoint, i: usize) -> F80 {
    F80::from_f64(xyzw(p)[i])
}
// (`inter` includes si-gen's list.rs by path; that file declares `mod tests;`, which a test build of this crate cannot find:
// with a `si-gen` dependency the include would become `use si_gen::list`)
#[cfg(not(test))]
pub mod inter;
pub mod trimstat;
#[cfg(not(test))]
pub mod curveinter;
#[cfg(not(test))]
pub mod curveinter2;
#[cfg(not(test))]
pub mod curveinter3;
pub mod composite2;
#[cfg(not(test))]
pub mod curveorient;
#[cfg(not(test))]
pub mod composite3;
#[cfg(not(test))]
pub mod trimprev;
pub mod elev;
pub mod conv6;
#[cfg(not(test))]
pub mod routed;
#[cfg(not(test))]
pub mod curveorient2;
#[cfg(not(test))]
pub mod trimutils2;
pub mod geom7;
pub mod geom8;
// nurbstess1 (finding 0025 section 24, 0044 section "Tess1")
pub mod flatten;
pub mod chgknots;
pub mod geom9;
#[cfg(not(test))]
pub mod raylink;
#[cfg(not(test))]
pub mod trimutils3;
pub mod geom10;
pub mod geom11;
pub mod sort;
pub mod split;
pub mod tess2;
pub mod ins2;
pub mod convert2;
pub mod dist;
pub mod geom13;
pub mod geom13sys;
pub mod close;
