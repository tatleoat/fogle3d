// SPDX-License-Identifier: MIT
//! The routines of `eval.c` that the conversion of a surface to triangles calls, in a second
//! form that works on the memory image (finding 0024; finding 0025, section 13).
//!
//! Each takes the image and the addresses the original takes, reads and writes the span
//! search's static where the binary has it (`left.272`, 0x086d9c20), makes the original's
//! allocation calls in the original's order (the scratch blocks of an order above 4
//! included, which the original takes from the heap and gives back before it returns), and
//! leaves its results where the original leaves them. The arithmetic is the first form's
//! (`eval`, `sample`), which the comparison with the emulated original holds bit for bit
//! (finding 0025, section 11); what the records are read from and written to is this
//! module's, and is checked on its own against the original stepped on surfaces of the
//! dumped sample scenes (finding 0025, section 13).
//!
//! One routine of `nrb_misc.c` has its image form here too: `SI_nurbsCrvMovePoint`
//! ([`crv_move_point`]), the point move of the update's shape blends (finding 0030); and four
//! of `nrb_geometry.c`, the path code's `SI_nurbsCurveLength` ([`nurbs_curve_length`]) and
//! `SI_nurbsDistanceToFactor` ([`nurbs_distance_to_factor`]), and what builds the length
//! tables they read, `SI_nurbsCurveUpdateLength` ([`nurbs_curve_update_length`]) with the
//! touch it asks for, `SI_nurbsCrvAnimUpdate` ([`nurbs_crv_anim_update`]: its calls into the
//! animation package go through `si_mem::Outside`).
//!
//! The copies, disposals and point transfers the update's envelope, shapes and readjusting
//! call have their image forms here too (finding 0025, section 13): `SI_nurbsCrvCopy`
//! ([`crv_copy`]), `SI_nurbsSrfCopy` ([`srf_copy`]) with the trim and projection copies,
//! `SI_nurbsSrfDispose` ([`srf_dispose`]) with the composites', `SI_nurbsSrfMovePoint`
//! ([`srf_move_point`]) with `SI_nurbsCapping`, `SI_nurbsSrfTransferPointsKnots`
//! ([`srf_transfer_points_knots`]) and the display lines' disposal
//! (`SI_nurbsSrfDisposeLines`, [`srf_dispose_lines`]); what they call outside these files
//! (`SI_genTreeDestroy`, `markDirtyCcrvs`, `SI_nurbsSrfForceCapping`) goes through
//! `si_mem::Outside`.
//!
//! What makes a face of NURBS curves linear before it is triangulated has its image form
//! here too (finding 0012, section 18.5): `SI_nurbsCrvConvertToLinear`
//! ([`crv_convert_to_linear`], `nrb_convert.c`) and the knots it makes,
//! `SI_knotsCrvReparametrization` ([`knots_crv_reparametrization`], `nrb_b.c`) with its three
//! helpers (`si_nurbsCrvFillDistancePoints`, `knotsCrvReparamChordCentri`,
//! `knotsCrvReparamChordCentriQuad`); checked against the emulated original on
//! `gen_convert.py`'s group `lin` (re/out/si-nurbs/vec17). So has the reversal the face's
//! contours are wound with (`SI_faceOrdinatePoints`): `SI_nurbsCrvInverse` ([`crv_inverse`])
//! with `si_nurbsDirInverse` ([`dir_inverse`]), checked on the group `inv`
//! (re/out/si-nurbs/vec18).
//!
//! The allocators and the curve's disposal on the image are `si-hrc`'s ports
//! (`SI_nurbsCrvAllocate`, `SI_nurbsPointsAllocate`, `si_knotsAllocate`,
//! `SI_nurbsCrvDispose`: `si_hrc::image::nurbs`), which this crate depends on.

use crate::{CrvRef, Dir, NurbsCrv, NurbsDir, NurbsPoint, NurbsSrf, SrfRef, Statics};
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_points_allocate};
use si_mem::{Mem, Outside, Ptr32, Routine};

pub use si_hrc::image::nurbs::nurbs_crv_dispose;

/// `left.272` (0x086d9c20), the static of `si_nurbsInterv`: the span it last returned.
pub const LEFT: u32 = 0x086d_9c20;

/// Why a routine did not return as the original would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    /// The first form refused: a span search that does not end (the original then loops for
    /// ever), a read outside the knots or the points the record names (the original reads
    /// whatever lies there), a count the original's arithmetic wraps. Named by the routine.
    Refused(&'static str),
    /// A routine that is not ported (named by the original's name).
    Unported(&'static str),
}

fn f64s(m: &Mem, a: u32, n: i32) -> Vec<f64> {
    if a == 0 || n <= 0 || n > 1 << 24 || !m.is_mapped(a, (n as u32) * 8) {
        return Vec::new();
    }
    (0..n as u32).map(|i| m.f64(a + 8 * i)).collect()
}

fn points(m: &Mem, a: u32, n: i64) -> Vec<NurbsPoint> {
    if a == 0 || n <= 0 || n > 1 << 22 || !m.is_mapped(a, n as u32 * 0x24) {
        return Vec::new();
    }
    (0..n as u32).map(|i| m.read(Ptr32::<NurbsPoint>::new(a + 0x24 * i))).collect()
}

/// A curve of the image as the first form sees it: the record and copies of its arrays
/// (the knots and points the record counts).
pub struct CrvHeld {
    pub rec: NurbsCrv,
    pub knots: Vec<f64>,
    pub points: Vec<NurbsPoint>,
}

impl CrvHeld {
    pub fn at(m: &Mem, a: u32) -> CrvHeld {
        let rec: NurbsCrv = m.read(Ptr32::new(a));
        let knots = f64s(m, rec.dir.knots.addr(), rec.dir.nb_knots);
        let points = points(m, rec.points.addr(), rec.dir.nb_points as i64);
        CrvHeld { rec, knots, points }
    }
    pub fn view(&self) -> CrvRef<'_> {
        CrvRef { rec: &self.rec, knots: &self.knots, points: &self.points }
    }
}

/// A surface of the image as the first form sees it: the record, its two knot vectors and
/// its `nu × nv` points (or the count at +0x48 when the product is not positive).
pub struct SrfHeld {
    pub rec: NurbsSrf,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub points: Vec<NurbsPoint>,
}

impl SrfHeld {
    pub fn at(m: &Mem, a: u32) -> SrfHeld {
        let rec: NurbsSrf = m.read(Ptr32::new(a));
        let knots_u = f64s(m, rec.u.knots.addr(), rec.u.nb_knots);
        let knots_v = f64s(m, rec.v.knots.addr(), rec.v.nb_knots);
        let grid = rec.u.nb_points as i64 * rec.v.nb_points as i64;
        let n = if grid > 0 { grid } else { rec.nb_points as i64 };
        let points = points(m, rec.points.addr(), n);
        SrfHeld { rec, knots_u, knots_v, points }
    }
    pub fn view(&self) -> SrfRef<'_> {
        SrfRef { rec: &self.rec, knots_u: &self.knots_u, knots_v: &self.knots_v, points: &self.points }
    }
    /// Lets the routines read the words beside the two knot arrays in the image, as the original does when a walk leaves an
    /// array (`SI_nurbsSrfExtractCurve*` reads the knots below the first when the span search ends at the start of a
    /// degenerate vector). Words outside mapped memory are not offered (the routine then refuses).
    pub fn margins(&self, m: &Mem) -> crate::MarginGuard {
        fn side<'k>(m: &Mem, ptr: u32, n: i32, ks: &'k [f64]) -> (&'k [f64], Vec<f64>, Vec<f64>) {
            let w = |a: u32| if m.is_mapped(a, 8) { Some(m.f64(a)) } else { None };
            let before: Vec<f64> = (1..=MARGIN_WORDS).map_while(|j| w(ptr.wrapping_sub(8 * j))).collect();
            let after: Vec<f64> = (0..MARGIN_WORDS).map_while(|j| w(ptr.wrapping_add(8u32.wrapping_mul((n.max(0) as u32).wrapping_add(j))))).collect();
            (ks, before, after)
        }
        crate::MarginGuard::new(vec![
            side(m, self.rec.u.knots.addr(), self.rec.u.nb_knots, &self.knots_u),
            side(m, self.rec.v.knots.addr(), self.rec.v.nb_knots, &self.knots_v),
        ])
    }
}

/// How many words beside a knot array [`SrfHeld::margins`] offers.
const MARGIN_WORDS: u32 = 32;

pub(crate) fn statics(m: &Mem) -> Statics {
    Statics { left: m.i32(LEFT) }
}

pub(crate) fn keep(m: &mut Mem, st: Statics) {
    m.set_i32(LEFT, st.left);
}

/// `free` of a block that is not null (the original tests each scratch pointer).
pub(crate) fn free_some(m: &mut Mem, p: u32) {
    if p != 0 {
        m.free(p);
    }
}

/// The scratch of a curve evaluator of order `k` above 4: two `SI_calloc(k − 1, 8)` (the
/// knot distances), `SI_nurbsPointsAllocate(k)` (the triangle); given back points first.
/// (Below order 5 the three live on the stack.) Their contents are the evaluator's
/// business; what the image keeps of them is the heap's bookkeeping.
fn scratch_taken(m: &mut Mem, k: i32) -> [u32; 3] {
    let dl = m.si_calloc(k.wrapping_sub(1) as u32, 8);
    let dr = m.si_calloc(k.wrapping_sub(1) as u32, 8);
    let p = nurbs_points_allocate(m, k);
    [dl, dr, p]
}

/// [soft 0x084d727c SI_SampleKnotVector], on the image: `SI_SampleKnotVector(dir, per,
/// &array, &count)`. Nothing is done for a null direction or one whose knot pointer is
/// null; otherwise `*count` is written, the array comes from `SI_calloc(count, 8)` and
/// `*array` points to it (what `*array` held before is not freed). The parameters are the
/// first form's ([`crate::sample::sample_knot_vector`]).
pub fn sample_knot_vector(m: &mut Mem, dir: u32, per: i32, array: u32, count: u32) -> Result<(), Halt> {
    if let Some((a, n)) = sampled(m, dir, per)? {
        m.set_i32(count, n);
        m.set_u32(array, a);
    }
    Ok(())
}

/// What [`sample_knot_vector`] makes, without the two stores through the caller's pointers:
/// the array (from `SI_calloc(count, 8)`) and its count; `None` where nothing is done.
pub(crate) fn sampled(m: &mut Mem, dir: u32, per: i32) -> Result<Option<(u32, i32)>, Halt> {
    if dir == 0 {
        return Ok(None);
    }
    let rec: NurbsDir = m.read(Ptr32::new(dir));
    if rec.knots.is_null() {
        return Ok(None);
    }
    let knots = f64s(m, rec.knots.addr(), rec.nb_knots);
    let d = Dir { rec: &rec, knots: &knots };
    match crate::sample::sample_knot_vector(Some(d), true, per) {
        Some(Some(v)) => {
            let a = m.si_calloc(v.len() as u32, 8);
            for (i, x) in v.iter().enumerate() {
                m.set_f64(a + 8 * i as u32, *x);
            }
            Ok(Some((a, v.len() as i32)))
        }
        _ => Err(Halt::Refused("SI_SampleKnotVector")),
    }
}

/// [soft 0x084d3ca0 SI_nurbsCrvValue4D], on the image: the homogeneous point of the curve
/// at `curve` at `t` (the original returns the 36-byte record through a hidden pointer;
/// here it is the answer).
pub fn crv_value4d(m: &mut Mem, curve: u32, t: f64) -> Result<NurbsPoint, Halt> {
    let c = CrvHeld::at(m, curve);
    let mut st = statics(m);
    let r = crate::eval::crv_value4d(&mut st, c.view(), t);
    keep(m, st);
    let p = r.ok_or(Halt::Refused("SI_nurbsCrvValue4D"))?;
    let k = c.rec.dir.order;
    if k > 4 {
        let [dl, dr, pts] = scratch_taken(m, k);
        free_some(m, pts);
        free_some(m, dl);
        free_some(m, dr);
    }
    Ok(p)
}

/// The curves of [`srf_value_and_normal`] on the heap, as the original makes them: `SI_nurbsSrfExtractCurveSeg`
/// ([`crate::geom8::extract_seg_heap`]), the two evaluators of the image and `SI_nurbsCrvDispose`.
struct HeapEnv<'a> {
    m: &'a mut Mem,
    held: &'a SrfHeld,
}

impl crate::eval::SvnEnv for HeapEnv<'_> {
    type C = u32;
    fn extract(&mut self, u: f64, v: f64, nd: i32) -> Option<u32> {
        let (c, ok) = crate::geom8::extract_seg_heap(self.m, self.held, u, v, nd);
        ok.then_some(c)
    }
    fn value4d(&mut self, c: &u32, t: f64) -> Option<NurbsPoint> {
        crv_value4d(self.m, *c, t).ok()
    }
    fn value(&mut self, c: &u32, t: f64, nd: i32) -> Option<[f64; 3]> {
        crv_value(self.m, *c, t, nd).ok()
    }
    fn dispose(&mut self, c: u32) {
        crate::geom8::dispose_curve(self.m, c);
    }
}

/// [soft 0x084d6824 SI_nurbsSrfValueAndNormal], on the image, for a caller that passes no
/// curves of its own (the last two arguments null, as `si_waveGeomUpdate` does): the point
/// (three doubles, which the original returns through a hidden pointer) to `out`, the unit
/// normal to `normal`; the arithmetic is [`crate::eval::srf_value_and_normal_with`], with the
/// original's heap traffic: the position and derivative curves it extracts
/// (`SI_nurbsSrfExtractCurveSeg`, its static scratch blocks grown and written), the evaluators'
/// own scratch, the extra extractions of the search for a vanished derivative, and the
/// `SI_nurbsCrvDispose` calls in the original's order. A refused evaluation writes nothing
/// (curves made so far stay).
pub fn srf_value_and_normal(m: &mut Mem, out: u32, srf: u32, u: f64, v: f64, normal: u32) {
    srf_value_and_normal_curves(m, out, srf, u, v, normal, 0, 0)
}

/// [`srf_value_and_normal`] with the caller's two curves `a` (the original's sixth argument) and `b` (the seventh), each null
/// or the address of a curve record on the image (trimtri1: `EvaluateVertices` passes the iso-curves it extracted with
/// `SI_nurbsSrfExtractCurve`). With both given they are evaluated instead of extracting, and neither is disposed; the branches
/// that hang on `a` alone and on both are [`crate::eval::srf_value_and_normal_with`]'s.
#[allow(clippy::too_many_arguments)]
pub fn srf_value_and_normal_curves(m: &mut Mem, out: u32, srf: u32, u: f64, v: f64, normal: u32, a: u32, b: u32) {
    let held = SrfHeld::at(m, srf);
    let _margins = held.margins(m);
    let (ga, gb) = ((a != 0).then_some(a), (b != 0).then_some(b));
    let r = crate::eval::srf_value_and_normal_with(&mut HeapEnv { m: &mut *m, held: &held }, held.view(), u, v, ga.as_ref(), gb.as_ref(), &mut [0; 2]);
    if let Some((p, n)) = r {
        for k in 0..3u32 {
            m.set_f64(out.wrapping_add(8 * k), p[k as usize]);
            m.set_f64(normal.wrapping_add(8 * k), n[k as usize]);
        }
    }
}

/// [soft 0x084d3550 SI_nurbsCrvValue], on the image: the `nd`-th derivative of the curve at
/// `curve` at `t` (three doubles, which the original returns through a hidden pointer).
/// Scratch from the heap: for an order above 4 that of [`crv_value4d`]; for `nd` above 3
/// also `SI_nurbsPointsAllocate(nd + 1)` and `SI_calloc(nd + 1, 0x18)`, given back in the
/// reverse order before the first three. The blocks are given back holding what the original
/// leaves in them, which a heap compare sees: the two knot-distance arrays, the de Boor table
/// as the last derivative order leaves it (all `order` points, 36 bytes each), the `nd + 1`
/// homogeneous derivatives (one 36-byte point each) and the `nd + 1` rational derivatives
/// (x, y, z, 0x18 bytes each).
pub fn crv_value(m: &mut Mem, curve: u32, t: f64, nd: i32) -> Result<[f64; 3], Halt> {
    let c = CrvHeld::at(m, curve);
    let mut st = statics(m);
    let r = crate::eval::crv_value_parts(&mut st, c.view(), t, nd);
    keep(m, st);
    let sc = r.ok_or(Halt::Refused("SI_nurbsCrvValue"))?;
    let k = c.rec.dir.order;
    let first = if k > 4 { Some(scratch_taken(m, k)) } else { None };
    if let Some([dl, dr, pts]) = first {
        for (i, v) in sc.dl.iter().enumerate() {
            m.set_f64(dl + 8 * i as u32, *v);
        }
        for (i, v) in sc.dr.iter().enumerate() {
            m.set_f64(dr + 8 * i as u32, *v);
        }
        for (i, v) in sc.table.iter().enumerate() {
            m.write(Ptr32::new(pts + 0x24 * i as u32), *v);
        }
    }
    if nd > 3 {
        let a = nurbs_points_allocate(m, nd.wrapping_add(1));
        let b = m.si_calloc(nd.wrapping_add(1) as u32, 0x18);
        for (i, v) in sc.derivs.iter().enumerate() {
            m.write(Ptr32::new(a + 0x24 * i as u32), *v);
        }
        for (i, v) in sc.rational.iter().enumerate() {
            for q in 0..3u32 {
                m.set_f64(b + 0x18 * i as u32 + 8 * q, v[q as usize]);
            }
        }
        free_some(m, b);
        free_some(m, a);
    }
    if let Some([dl, dr, pts]) = first {
        free_some(m, pts);
        free_some(m, dl);
        free_some(m, dr);
    }
    Ok(sc.value)
}

/// The two whole-curve extractions on the image. `across_v` false:
/// [soft 0x084d52a4 SI_nurbsSrfExtractCurveU] (the curve in V at the parameter `t` of U),
/// true: [soft 0x084d5964 SI_nurbsSrfExtractCurveV] (the curve in U at `t` of V).
///
/// The original's calls, in order: the span search; `SI_nurbsCrvAllocate`;
/// `SI_nurbsPointsAllocate(rows)` (the other direction's point count) into +0x2c; for an
/// order of the reduced direction `k` above 4 its scratch, `SI_calloc(k − 1, 8)` twice,
/// `SI_nurbsPointsAllocate(k)` and `SI_calloc((k − 1) × k / 2, 8)` (the factors), given back
/// factors, points, then the two distance arrays; then +0x34 (the other direction's step:
/// +0x74 for U, +0x70 for V), +0x24 (the surface's +0x4c), order and counts, `si_knotsAllocate`
/// into +0x0c, the knots copied, the range and the closed word.
fn extract(m: &mut Mem, srf: u32, t: f64, nd: i32, across_v: bool) -> Result<u32, Halt> {
    let s = SrfHeld::at(m, srf);
    let _margins = s.margins(m);
    let mut st = statics(m);
    let r = if across_v { crate::eval::srf_extract_curve_v(&mut st, s.view(), t, nd) } else { crate::eval::srf_extract_curve_u(&mut st, s.view(), t, nd) };
    keep(m, st);
    let name = if across_v { "SI_nurbsSrfExtractCurveV" } else { "SI_nurbsSrfExtractCurveU" };
    let c = r.ok_or(Halt::Refused(name))?;
    let (reduced, other) = if across_v { (&s.rec.v, &s.rec.u) } else { (&s.rec.u, &s.rec.v) };
    let k = reduced.order;
    let a = nurbs_crv_allocate(m).addr();
    let pts = nurbs_points_allocate(m, other.nb_points);
    m.set_u32(a + 0x2c, pts);
    if k > 4 {
        let dl = m.si_calloc(k.wrapping_sub(1) as u32, 8);
        let dr = m.si_calloc(k.wrapping_sub(1) as u32, 8);
        let p = nurbs_points_allocate(m, k);
        let f = m.si_calloc((k.wrapping_sub(1).wrapping_mul(k) / 2) as u32, 8);
        free_some(m, f);
        free_some(m, p);
        free_some(m, dl);
        free_some(m, dr);
    }
    if let Some(cp) = &c.points {
        for (j, p) in cp.iter().enumerate().take(other.nb_points.max(0) as usize) {
            m.write(Ptr32::new(pts + 0x24 * j as u32), *p);
        }
    }
    m.set_i32(a + 0x34, c.rec.step);
    m.set_i32(a + 0x24, c.rec.param);
    m.set_i32(a, other.order);
    m.set_i32(a + 4, other.nb_points);
    m.set_i32(a + 8, other.nb_knots);
    let kn = knots_allocate(m, other.nb_knots);
    m.set_u32(a + 0x0c, kn);
    if let Some(ck) = &c.knots {
        for (i, x) in ck.iter().enumerate().take(other.nb_knots.max(0) as usize) {
            m.set_f64(kn + 8 * i as u32, *x);
        }
    }
    m.set_f64(a + 0x10, other.range_start.0);
    m.set_f64(a + 0x18, other.range_end.0);
    m.set_i16(a + 0x20, other.closed);
    Ok(a)
}

pub fn srf_extract_curve_u(m: &mut Mem, srf: u32, u: f64, nd: i32) -> Result<u32, Halt> {
    extract(m, srf, u, nd, false)
}

pub fn srf_extract_curve_v(m: &mut Mem, srf: u32, v: f64, nd: i32) -> Result<u32, Halt> {
    extract(m, srf, v, nd, true)
}

/// [soft 0x084d5268 SI_nurbsSrfExtractCurve], on the image: `dir` 0 is
/// [`srf_extract_curve_u`], anything else [`srf_extract_curve_v`]. Returns the new curve.
pub fn srf_extract_curve(m: &mut Mem, srf: u32, t: f64, dir: i32, nd: i32) -> Result<u32, Halt> {
    if dir == 0 {
        srf_extract_curve_u(m, srf, t, nd)
    } else {
        srf_extract_curve_v(m, srf, t, nd)
    }
}

/// [soft 0x084e9eec SI_nurbsCrvMovePoint], on the image (`nrb_misc.c`; the first form is
/// [`crate::misc::crv_move_point`], the same arithmetic): `SI_nurbsCrvMovePoint(curve, i, x,
/// y, z)`. Point `i` of the curve's array (+0x2c, 0x24 bytes a point; the index is not
/// tested and the address wraps as the original's 32-bit arithmetic does): its weight
/// (+0x18) is read, bits 0x60 of its flag word's highest byte (+0x23) are set, then x, y and
/// z times the weight are stored (each product in the register, one rounding to `double`);
/// last, bits 0x60 of the curve's +0x3b. Returns 1.
pub fn crv_move_point(m: &mut Mem, curve: u32, i: i32, x: f64, y: f64, z: f64) -> i32 {
    let p = m.u32(curve.wrapping_add(0x2c)).wrapping_add((i as u32).wrapping_mul(0x24));
    let w = si_x87::F80::from_f64(m.f64(p.wrapping_add(0x18)));
    let flag = m.u8(p.wrapping_add(0x23));
    m.set_u8(p.wrapping_add(0x23), flag | 0x60);
    for (k, v) in [x, y, z].into_iter().enumerate() {
        m.set_f64(p.wrapping_add(8 * k as u32), (si_x87::F80::from_f64(v) * w).to_f64());
    }
    let c = m.u8(curve.wrapping_add(0x3b));
    m.set_u8(curve.wrapping_add(0x3b), c | 0x60);
    1
}

/// [soft 0x084e9750 SI_nurbsCrvApplyMatrix], on the image (`nrb_misc.c`; the first form is
/// [`crate::misc::crv_apply_matrix`], the same arithmetic): `SI_nurbsCrvApplyMatrix(curve, m,
/// mask)`. The point pointer (+0x2c) is read once; the count (+4) is read again after each
/// point. A point is transformed when `mask` is 0 or its flag word (+0x20) has one of the
/// mask's bits: x, y, z, w read, then each coordinate q is stored as
/// `((x*m[0][q] + y*m[1][q]) + z*m[2][q]) + w*m[3][q]` in the register (the floats widened
/// exactly, one rounding to `double`), bits 0x60 of the flag word's highest byte (+0x23) set.
/// Last, bits 0x60 of the curve's +0x3b, whether or not a point was transformed.
pub fn crv_apply_matrix(m: &mut Mem, curve: u32, mat: u32, mask: i32) {
    use si_x87::F80;
    let mut p = m.u32(curve.wrapping_add(0x2c));
    let mut i: i32 = 0;
    if 0 < m.i32(curve.wrapping_add(4)) {
        loop {
            if mask == 0 || (m.u32(p.wrapping_add(0x20)) & mask as u32) != 0 {
                let v: [F80; 4] = core::array::from_fn(|k| F80::from_f64(m.f64(p.wrapping_add(8 * k as u32))));
                let out: [f64; 4] = core::array::from_fn(|q| {
                    let f = |r: u32| F80::from_f32(m.f32(mat.wrapping_add(4 * (4 * r + q as u32))));
                    (((v[0] * f(0) + v[1] * f(1)) + v[2] * f(2)) + v[3] * f(3)).to_f64()
                });
                for (q, x) in out.into_iter().enumerate() {
                    m.set_f64(p.wrapping_add(8 * q as u32), x);
                }
                let flag = m.u8(p.wrapping_add(0x23));
                m.set_u8(p.wrapping_add(0x23), flag | 0x60);
            }
            i = i.wrapping_add(1);
            p = p.wrapping_add(0x24);
            if i >= m.i32(curve.wrapping_add(4)) {
                break;
            }
        }
    }
    let c = m.u8(curve.wrapping_add(0x3b));
    m.set_u8(curve.wrapping_add(0x3b), c | 0x60);
}

fn refused(r: crate::geometry::Refusal) -> Halt {
    match r {
        crate::geometry::Refusal::Unbuilt | crate::geometry::Refusal::Unsampled | crate::geometry::Refusal::Unevaluated => Halt::Refused("SI_nurbsCurveUpdateLength"),
        _ => Halt::Refused("SI_nurbsDistanceToFactor"),
    }
}

/// `SI_procData` (0x08679314): points at the record whose second word is the number of the
/// current object list.
pub const SI_PROC_DATA: u32 = 0x0867_9314;
/// `SI_grafLists` (0x08679498): points at the object lists, 0x24 bytes each; a list's first
/// node at +4.
pub const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// The animation package's routines the touch of a keyframed curve calls.
pub const SI_FIND_NURBS_PATH_FCURVE_OWNER: Routine = Routine::new("SI_findNURBSPathFcurveOwner", 0x0821_f7e4);
pub const SI_UPDATE_FCURVE_NURBS_PATH_KEYS: Routine = Routine::new("SI_updateFcurveNURBSPathKeys", 0x0821_fd94);
pub const SI_CHECK_NRB_CRV_TO_FCURVE: Routine = Routine::new("SI_checkNrbCrvToFcurve", 0x0821_fca0);
pub const SI_REFRESH_FCURVE: Routine = Routine::new("SI_refreshFcurve", 0x0832_3ce0);

/// Frees the block a word points at when it is not null, and clears the word; then clears
/// it again (the original's `if (p) { free(p); p = 0; } p = 0;`).
fn free_word(m: &mut Mem, at: u32) {
    let p = m.u32(at);
    if p != 0 {
        m.free(p);
        m.set_u32(at, 0);
    }
    m.set_u32(at, 0);
}

/// Marks a NURBS curve as changed [soft 0x08501d90 SI_nurbsCrvAnimUpdate] (the curve is not
/// tested for null):
///
/// 1. A count of length entries (+0x50) that is not 0 is cleared, and the cumulated lengths
///    (+0x58) freed and their pointer cleared (the parameters at +0x54 stay); bit 0x10 of
///    the byte at +0x38.
/// 2. The walk over the current object list (`SI_grafLists[SI_procData[1]]`, from its first
///    node along +0x1c): for a node of kind 2 or 3 with `model` its data (+4): when the
///    model is a NURBS curve (kind 9 at +0x30) whose definition (+0x48) is this curve, bit
///    0x10 of both update flag bytes (+0x0a, +0x3fa); for each of its two spline
///    deformation records (+0x60, +0x64) whose path model's (+4) definition is this curve,
///    the record's table (+8) freed and cleared.
/// 3. For a keyframed curve (+0x4c is 1): `SI_findNURBSPathFcurveOwner(curve)`,
///    `SI_updateFcurveNURBSPathKeys(owner, curve)`, `SI_checkNrbCrvToFcurve(owner, curve)`,
///    `SI_refreshFcurve(0)`, through `o`.
pub fn nurbs_crv_anim_update(m: &mut Mem, o: &mut dyn Outside, curve: u32) {
    if m.i32(curve.wrapping_add(0x50)) != 0 {
        m.set_i32(curve.wrapping_add(0x50), 0);
        free_word(m, curve.wrapping_add(0x58));
    }
    m.or_u8(curve.wrapping_add(0x38), 0x10);
    let current = m.u32(m.u32(SI_PROC_DATA).wrapping_add(4));
    let mut node = m.u32(m.u32(SI_GRAF_LISTS).wrapping_add(current.wrapping_mul(0x24)).wrapping_add(4));
    while node != 0 {
        if m.u32(node).wrapping_sub(2) <= 1 {
            let model = m.u32(node.wrapping_add(4));
            if m.i32(model.wrapping_add(0x30)) == 9 && m.u32(model.wrapping_add(0x48)) == curve {
                m.or_u8(model.wrapping_add(0x0a), 0x10);
                m.or_u8(model.wrapping_add(0x3fa), 0x10);
            }
            for at in [0x60u32, 0x64] {
                let fit = m.u32(model.wrapping_add(at));
                if fit != 0 && m.u32(m.u32(fit.wrapping_add(4)).wrapping_add(0x48)) == curve {
                    free_word(m, fit.wrapping_add(8));
                }
            }
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    if m.i16(curve.wrapping_add(0x4c)) == 1 {
        let owner = o.call(m, SI_FIND_NURBS_PATH_FCURVE_OWNER, &[curve]).eax;
        o.call(m, SI_UPDATE_FCURVE_NURBS_PATH_KEYS, &[owner, curve]);
        o.call(m, SI_CHECK_NRB_CRV_TO_FCURVE, &[owner, curve]);
        o.call(m, SI_REFRESH_FCURVE, &[0]);
    }
}

/// [soft 0x08501ed8 SI_nurbsCurveUpdateLength], on the image (the first form, with the
/// steps, is [`crate::geometry::curve_update_length`]): nothing for a null curve; `flag` 1
/// sets bits 0x60 of +0x3b and calls [`nurbs_crv_anim_update`]. Then, in the original's
/// order: `SI_SampleKnotVector(curve, samples)`'s `SI_calloc`; the old parameters (+0x54)
/// and cumulated lengths (+0x58) freed; the new parameters and their count stored;
/// `SI_calloc(count, 8)` for the cumulated lengths, entry 0 set to 0; then
/// `SI_nurbsCrvValue(curve, t, 0)` at every parameter ([`crv_value`], with its scratch for
/// an order above 4), each entry stored as it is computed
/// ([`crate::geometry::chord_add`]). A curve without knots (the original then reads through
/// a null array with a count it never set) or a sampling or evaluation the first form
/// refuses is `Halt::Refused` (by the refusing routine's name), except the evaluation at
/// the one parameter of a curve of one sample, whose point is not used.
pub fn nurbs_curve_update_length(m: &mut Mem, o: &mut dyn Outside, curve: u32, samples: i32, flag: i16) -> Result<(), Halt> {
    if curve == 0 {
        return Ok(());
    }
    if flag == 1 {
        m.or_u8(curve.wrapping_add(0x3b), 0x60);
        nurbs_crv_anim_update(m, o, curve);
    }
    let (params, count) = sampled(m, curve, samples)?.ok_or(Halt::Refused("SI_nurbsCurveUpdateLength"))?;
    free_word(m, curve.wrapping_add(0x54));
    free_word(m, curve.wrapping_add(0x58));
    m.set_u32(curve.wrapping_add(0x54), params);
    m.set_i32(curve.wrapping_add(0x50), count);
    let table = m.si_calloc(count as u32, 8);
    m.set_u32(curve.wrapping_add(0x58), table);
    m.set_f64(table, 0.0);
    // the first point is used only from the second sample on: a curve of one sample (all
    // its knots equal, say) is not refused for an evaluation that reads outside its knots
    let first = crv_value(m, curve, m.f64(params), 0);
    if count == 1 {
        return Ok(());
    }
    let mut p = first?;
    for i in 1..count as u32 {
        let q = crv_value(m, curve, m.f64(params + 8 * i), 0)?;
        let v = crate::geometry::chord_add(m.f64(table + 8 * (i - 1)), p, q);
        m.set_f64(table + 8 * i, v);
        p = q;
    }
    Ok(())
}

/// [soft 0x08502070 SI_nurbsCurveLength], on the image (`nrb_geometry.c`; the first form is
/// [`crate::geometry::curve_length`]): 0 for a null curve; a count (+0x50) of 0 first has
/// [`nurbs_curve_update_length`]`(curve, 100, 1)` build the tables; then the double at
/// `+0x58 + 8 × (count − 1)` (the address wraps as the original's 32-bit arithmetic does)
/// as loaded into the unit.
pub fn nurbs_curve_length(m: &mut Mem, o: &mut dyn Outside, curve: u32) -> Result<si_x87::F80, Halt> {
    if curve != 0 && m.i32(curve.wrapping_add(0x50)) == 0 {
        nurbs_curve_update_length(m, o, curve, 100, 1)?;
    }
    let count = if curve == 0 { 0 } else { m.i32(curve.wrapping_add(0x50)) };
    let at = |i: i32| Some(m.f64(m.u32(curve.wrapping_add(0x58)).wrapping_add((i as u32).wrapping_mul(8))));
    crate::geometry::curve_length_with(curve != 0, count, at).map_err(refused)
}

/// [soft 0x08502f4c SI_nurbsDistanceToFactor], on the image (the first form, with the
/// steps, is [`crate::geometry::distance_to_factor`]): the curve is not tested for null;
/// the length is [`nurbs_curve_length`]'s (which builds the tables when there are none);
/// the tables are read where +0x54 and +0x58 point, entry `i − 1` of the first entry above
/// the distance in front of them when that is entry 0. Refused
/// (`Halt::Refused("SI_nurbsDistanceToFactor")`) where the original loops for ever or reads
/// through the caller's `esi` ([`crate::geometry::Refusal`]).
pub fn nurbs_distance_to_factor(m: &mut Mem, o: &mut dyn Outside, curve: u32, distance: f64) -> Result<si_x87::F80, Halt> {
    if crate::geometry::near_start(distance) {
        return Ok(si_x87::F80::from_f64(m.f64(curve.wrapping_add(0x10))));
    }
    let len = nurbs_curve_length(m, o, curve)?;
    let m: &Mem = m;
    let ends = crate::geometry::Ends { closed: m.i16(curve.wrapping_add(0x20)), start: m.f64(curve.wrapping_add(0x10)), end: m.f64(curve.wrapping_add(0x18)) };
    let table = |off: u32, i: i32| Some(m.f64(m.u32(curve.wrapping_add(off)).wrapping_add((i as u32).wrapping_mul(8))));
    crate::geometry::distance_to_factor_with(&ends, distance, len, || m.i32(curve.wrapping_add(0x50)), |i| table(0x58, i), |i| table(0x54, i)).map_err(refused)
}

// ------------------------------------------------------------------------------ copies, disposals, the surface's point move

/// `SI_genTreeDestroy (0x08515848)`, the intersection package's: frees the bounding-box tree a
/// surface's +0xd8 points to (nothing for a null tree), with `SI_bezPatchDestroy` for its
/// elements. Called through `si_mem::Outside` with (`&srf->tree`, [`SI_BEZ_PATCH_DESTROY`]).
pub const SI_GEN_TREE_DESTROY: Routine = Routine::new("SI_genTreeDestroy", 0x0851_5848);
/// `SI_bezPatchDestroy (0x08503504)`: the destructor every call of [`SI_GEN_TREE_DESTROY`]
/// here hands over (its address is the argument word).
pub const SI_BEZ_PATCH_DESTROY: u32 = 0x0850_3504;
/// `markDirtyCcrvs (0x084ea5ec)`, static in `nrb_misc.c`, ported on the image as [`crate::geom5::mark_dirty_ccrvs`]
/// (checked, finding 0025 section 20); routing can answer this call with it. It marks the display
/// lines a moved point of an untrimmed surface invalidates (with `getPointInterval`,
/// `dirtyCurves`, `updateStartEnd`; it changes nothing but the line records at +0x84, and
/// divides by `nu` and by the knot counts less one). Called through `si_mem::Outside` with
/// (surface, point index).
pub const MARK_DIRTY_CCRVS: Routine = Routine::new("markDirtyCcrvs", 0x084e_a5ec);
/// `SI_nurbsSrfForceCapping (0x084f0134)`, `nrb_point.c` (ported on the image as [`srf_force_capping`]; the hook through `si_mem::Outside` stays for callers that route it): makes one border of a
/// surface a cap. Called through `si_mem::Outside` with (surface, direction 0 to 3, 0).
pub const SI_NURBS_SRF_FORCE_CAPPING: Routine = Routine::new("SI_nurbsSrfForceCapping", 0x084f_0134);

/// A `double` moved through the unit (`fld m64`, `fstp m64`): a signalling NaN comes out
/// quiet.
pub(crate) fn mov64(m: &mut Mem, from: u32, to: u32) {
    let v = si_x87::fmov64(m.f64(from));
    m.set_f64(to, v);
}

fn mov32(m: &mut Mem, from: u32, to: u32) {
    let v = m.u32(from);
    m.set_u32(to, v);
}

fn mov16(m: &mut Mem, from: u32, to: u32) {
    let v = m.u16(from);
    m.set_u16(to, v);
}

/// One control point (nine words, `rep movs`).
pub(crate) fn point_copy(m: &mut Mem, from: u32, to: u32) {
    for k in 0..9u32 {
        mov32(m, from.wrapping_add(4 * k), to.wrapping_add(4 * k));
    }
}

/// Frees the block the word at `at` points to when it is not null, and clears the word (the
/// original's `if (p) { free(p); p = 0; }`).
pub(crate) fn free_clear(m: &mut Mem, at: u32) {
    let p = m.u32(at);
    if p != 0 {
        m.free(p);
        m.set_u32(at, 0);
    }
}

/// [soft 0x084dbbf4 SI_nurbsCrvCopy], on the image (the first form is
/// [`crate::own::crv_copy`]): a new curve (`SI_nurbsCrvAllocate`; 0 when that fails) with
/// the source's order, counts, range, flags and steps; `si_knotsAllocate(knots)` and the
/// knots when the count is above 0 (copied up to the new record's count, the source's array
/// read again at every knot); `SI_nurbsPointsAllocate` and the points (nine words each) when
/// the **source's** point count is above 0; +0x48 cleared, +0x4e copied; with length tables
/// (+0x50 above 0) the count, `SI_calloc(count, 8)` into +0x58, then into +0x54, and the two
/// tables copied entry by entry. Doubles go through the unit (`fld`, `fstp`).
pub fn crv_copy(m: &mut Mem, src: u32) -> u32 {
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return 0;
    }
    for off in [0u32, 4, 8] {
        mov32(m, src + off, c + off);
    }
    if m.i32(c + 8) > 0 {
        let k = knots_allocate(m, m.i32(c + 8));
        m.set_u32(c + 0x0c, k);
        let mut i: i32 = 0;
        while i < m.i32(c + 8) {
            let (from, to) = (m.u32(src + 0x0c).wrapping_add(8 * i as u32), m.u32(c + 0x0c).wrapping_add(8 * i as u32));
            mov64(m, from, to);
            i += 1;
        }
    }
    mov64(m, src + 0x10, c + 0x10);
    mov64(m, src + 0x18, c + 0x18);
    mov16(m, src + 0x20, c + 0x20);
    mov16(m, src + 0x28, c + 0x28);
    mov32(m, src + 0x34, c + 0x34);
    mov32(m, src + 0x30, c + 0x30);
    mov32(m, src + 0x24, c + 0x24);
    mov16(m, src + 0x44, c + 0x44);
    mov32(m, src + 0x38, c + 0x38);
    mov64(m, src + 0x3c, c + 0x3c);
    mov16(m, src + 0x4c, c + 0x4c);
    if m.i32(src + 4) > 0 {
        let p = nurbs_points_allocate(m, m.i32(c + 4));
        m.set_u32(c + 0x2c, p);
        let mut i: i32 = 0;
        while i < m.i32(c + 4) {
            let off = 0x24u32.wrapping_mul(i as u32);
            let (from, to) = (m.u32(src + 0x2c).wrapping_add(off), m.u32(c + 0x2c).wrapping_add(off));
            point_copy(m, from, to);
            i += 1;
        }
    }
    m.set_u32(c + 0x48, 0);
    mov16(m, src + 0x4e, c + 0x4e);
    let n = m.i32(src + 0x50);
    if n > 0 {
        m.set_i32(c + 0x50, n);
        let d = m.si_calloc(m.u32(c + 0x50), 8);
        m.set_u32(c + 0x58, d);
        let p = m.si_calloc(m.u32(c + 0x50), 8);
        m.set_u32(c + 0x54, p);
        let mut i: i32 = 0;
        while i < m.i32(c + 0x50) {
            for off in [0x58u32, 0x54] {
                let (from, to) = (m.u32(src + off).wrapping_add(8 * i as u32), m.u32(c + off).wrapping_add(8 * i as u32));
                mov64(m, from, to);
            }
            i += 1;
        }
    }
    c
}

/// The common part of [`srf_trm_cpy`] and [`srf_proj_cpy`] after the border word: the count
/// (`cnt`) copied; for a count above 0, `SI_calloc(count, 4)` into `arr`, and for every
/// composite `SI_malloc(0x18)`, its curve count copied, and for a count above 0
/// `SI_calloc(curves, 4)` into +0x08, the boundary word (+0x00) copied and every curve
/// copied ([`crv_copy`]); else +0x08 cleared; then +0x0c, +0x10 and +0x14 cleared. Every
/// pointer is read again where the original reads it again.
fn comp_list_copy(m: &mut Mem, dst: u32, src: u32, cnt: u32, arr: u32) {
    let n = m.i32(src + cnt);
    m.set_i32(dst + cnt, n);
    if n <= 0 {
        return;
    }
    let a = m.si_calloc(n as u32, 4);
    m.set_u32(dst + arr, a);
    let mut i: i32 = 0;
    while i < m.i32(dst + cnt) {
        let slot = |m: &Mem, s: u32| m.u32(m.u32(s + arr).wrapping_add(4 * i as u32));
        let c = m.si_malloc(0x18);
        let at = m.u32(dst + arr).wrapping_add(4 * i as u32);
        m.set_u32(at, c);
        let k = m.i32(slot(m, src).wrapping_add(4));
        m.set_i32(slot(m, dst).wrapping_add(4), k);
        if k > 0 {
            let ca = m.si_calloc(m.u32(slot(m, dst).wrapping_add(4)), 4);
            m.set_u32(slot(m, dst).wrapping_add(8), ca);
            let b = m.u16(slot(m, src));
            m.set_u16(slot(m, dst), b);
            let mut j: i32 = 0;
            while j < m.i32(slot(m, dst).wrapping_add(4)) {
                let from = m.u32(m.u32(slot(m, src).wrapping_add(8)).wrapping_add(4 * j as u32));
                let cv = crv_copy(m, from);
                let to = m.u32(slot(m, dst).wrapping_add(8)).wrapping_add(4 * j as u32);
                m.set_u32(to, cv);
                j += 1;
            }
        } else {
            m.set_u32(slot(m, dst).wrapping_add(8), 0);
        }
        for off in [0x0cu32, 0x10, 0x14] {
            m.set_u32(slot(m, dst).wrapping_add(off), 0);
        }
        i += 1;
    }
}

/// [soft 0x084dbf08 SI_nurbsSrfTrmCpy], on the image (the first form is
/// [`crate::own::srf_trm_cpy`]): `dst`'s display lines disposed of ([`srf_dispose_lines`]),
/// the border word (+0x5c) copied, then the trims (+0x60, +0x64) as [`comp_list_copy`] says.
pub fn srf_trm_cpy(m: &mut Mem, o: &mut dyn Outside, dst: u32, src: u32) {
    srf_dispose_lines(m, o, dst);
    mov16(m, src + 0x5c, dst + 0x5c);
    comp_list_copy(m, dst, src, 0x60, 0x64);
}

/// [soft 0x084dc058 SI_nurbsSrfProjCpy], on the image ([`crate::own::srf_proj_cpy`]): the
/// border word, then the projected curves (+0x68, +0x6c); no display lines disposed of.
pub fn srf_proj_cpy(m: &mut Mem, dst: u32, src: u32) {
    mov16(m, src + 0x5c, dst + 0x5c);
    comp_list_copy(m, dst, src, 0x68, 0x6c);
}

/// [soft 0x084dc1a8 SI_nurbsSrfCopy], on the image (the first form is
/// [`crate::own::srf_copy`]): a new surface (`SI_nurbsSrfAllocate`; 0 when that fails); the U
/// direction's order and counts, +0x58, the U knots (`si_knotsAllocate` when the count is
/// above 0), range and closed word; the same for V; +0x44, +0x50, +0x4c, the steps and curve
/// counts (+0x70 to +0x7c); the point total (+0x48) and, above 0, `SI_nurbsPointsAllocate`
/// and the points; the trims ([`srf_trm_cpy`], whose disposal of the new surface's display
/// lines goes to `o`'s `SI_genTreeDestroy`) and the projected curves ([`srf_proj_cpy`]); the
/// capping words (+0x88 to +0x8e), +0x90, +0x94, +0x98, +0x9c, the three approximation
/// floats (through the unit) and +0xac to +0xb0.
pub fn srf_copy(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    let d = si_hrc::image::nurbs::nurbs_srf_allocate(m).addr();
    if d == 0 {
        return 0;
    }
    for off in [0u32, 4, 8, 0x58, 8] {
        mov32(m, src + off, d + off);
    }
    let knots = |m: &mut Mem, cnt: u32, arr: u32| {
        if m.i32(d + cnt) > 0 {
            let k = knots_allocate(m, m.i32(d + cnt));
            m.set_u32(d + arr, k);
            let mut i: i32 = 0;
            while i < m.i32(d + cnt) {
                let (from, to) = (m.u32(src + arr).wrapping_add(8 * i as u32), m.u32(d + arr).wrapping_add(8 * i as u32));
                mov64(m, from, to);
                i += 1;
            }
        }
    };
    knots(m, 8, 0x0c);
    mov64(m, src + 0x10, d + 0x10);
    mov64(m, src + 0x18, d + 0x18);
    mov16(m, src + 0x20, d + 0x20);
    for off in [0x24u32, 0x28, 0x2c] {
        mov32(m, src + off, d + off);
    }
    knots(m, 0x2c, 0x30);
    mov64(m, src + 0x34, d + 0x34);
    mov64(m, src + 0x3c, d + 0x3c);
    mov16(m, src + 0x44, d + 0x44);
    mov16(m, src + 0x50, d + 0x50);
    for off in [0x4cu32, 0x70, 0x74, 0x78, 0x7c, 0x48] {
        mov32(m, src + off, d + off);
    }
    if m.i32(d + 0x48) > 0 {
        let p = nurbs_points_allocate(m, m.i32(d + 0x48));
        m.set_u32(d + 0x54, p);
        let mut i: i32 = 0;
        while i < m.i32(d + 0x48) {
            let off = 0x24u32.wrapping_mul(i as u32);
            let (from, to) = (m.u32(src + 0x54).wrapping_add(off), m.u32(d + 0x54).wrapping_add(off));
            point_copy(m, from, to);
            i += 1;
        }
    }
    srf_trm_cpy(m, o, d, src);
    srf_proj_cpy(m, d, src);
    for off in [0x88u32, 0x8a, 0x8c, 0x8e] {
        mov16(m, src + off, d + off);
    }
    mov32(m, src + 0x90, d + 0x90);
    mov32(m, src + 0x94, d + 0x94);
    mov16(m, src + 0x98, d + 0x98);
    mov16(m, src + 0x9c, d + 0x9c);
    for off in [0xa0u32, 0xa4, 0xa8] {
        let v = si_x87::fmov(m.f32(src + off));
        m.set_f32(d + off, v);
    }
    for off in [0xacu32, 0xae, 0xb0] {
        mov16(m, src + off, d + off);
    }
    d
}

/// [soft 0x084e5468 SI_nurbsSrfDisposeLines], on the image (`nrb_draw.c`): for each of the
/// +0x80 display lines (20 bytes each, from +0x84) the block at +0x10 freed and cleared; the
/// array freed; +0x84 and +0x80 cleared; then `SI_genTreeDestroy(&srf->tree,
/// SI_bezPatchDestroy)` through `o`.
pub fn srf_dispose_lines(m: &mut Mem, o: &mut dyn Outside, srf: u32) {
    let mut i: i32 = 0;
    let mut off: u32 = 0;
    while i < m.i32(srf + 0x80) {
        free_clear(m, m.u32(srf + 0x84).wrapping_add(off).wrapping_add(0x10));
        off = off.wrapping_add(0x14);
        i += 1;
    }
    free_clear(m, srf + 0x84);
    m.set_u32(srf + 0x84, 0);
    m.set_u32(srf + 0x80, 0);
    o.call(m, SI_GEN_TREE_DESTROY, &[srf + 0xd8, SI_BEZ_PATCH_DESTROY]);
}

/// [soft 0x084dbb14 SI_nurbsCompDispose], on the image ([`crate::own::comp_dispose`]):
/// nothing for a null `*pp`; with a curve array, every curve (`SI_nurbsCrvDispose`, si-hrc's)
/// and the array; the trim line's nodes (linked at +0x14 from +0x08) and the line; the block
/// at +0x14; the record, and `*pp` cleared.
pub fn comp_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    if m.u32(m.u32(pp) + 8) != 0 {
        let mut i: i32 = 0;
        while i < m.i32(m.u32(pp) + 4) {
            nurbs_crv_dispose(m, m.u32(m.u32(pp) + 8).wrapping_add(4 * i as u32));
            i += 1;
        }
        free_clear(m, m.u32(pp) + 8);
    }
    let line = m.u32(m.u32(pp) + 0x0c);
    if line != 0 {
        let mut node = m.u32(line + 8);
        while node != 0 {
            let next = m.u32(node.wrapping_add(0x14));
            m.free(node);
            node = next;
        }
        free_clear(m, m.u32(pp) + 0x0c);
    }
    free_clear(m, m.u32(pp) + 0x14);
    free_clear(m, pp);
}

/// [soft 0x084db944 SI_nurbsSrfTrmDispose] (`cnt` 0x60, `arr` 0x64) and
/// [soft 0x084db9d0 SI_nurbsSrfProjDispose] (0x68, 0x6c), on the image: the display lines
/// disposed of with the bounding-box tree (+0xd8) hidden around the call; with an array,
/// every composite ([`comp_dispose`]) and the array; the count cleared.
fn srf_list_dispose(m: &mut Mem, o: &mut dyn Outside, pp: u32, cnt: u32, arr: u32) {
    let s = m.u32(pp);
    let tree = m.u32(s + 0xd8);
    m.set_u32(s + 0xd8, 0);
    srf_dispose_lines(m, o, m.u32(pp));
    m.set_u32(m.u32(pp) + 0xd8, tree);
    if m.u32(m.u32(pp) + arr) != 0 {
        let mut i: i32 = 0;
        while i < m.i32(m.u32(pp) + cnt) {
            comp_dispose(m, m.u32(m.u32(pp) + arr).wrapping_add(4 * i as u32));
            i += 1;
        }
        free_clear(m, m.u32(pp) + arr);
    }
    m.set_u32(m.u32(pp) + cnt, 0);
}

/// [soft 0x084dba5c SI_nurbsSrfDispose], on the image (the first form is
/// [`crate::own::srf_dispose`]): nothing for a null `*pp`; the U knots, V knots and points
/// freed and their words cleared; the trims ([soft 0x084db944 SI_nurbsSrfTrmDispose]) and the
/// projected curves ([soft 0x084db9d0 SI_nurbsSrfProjDispose]), each disposing of the display
/// lines first; `SI_genTreeDestroy(&srf->tree, SI_bezPatchDestroy)` through `o`; the record
/// freed and `*pp` cleared.
pub fn srf_dispose(m: &mut Mem, o: &mut dyn Outside, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    for off in [0x0cu32, 0x30, 0x54] {
        free_clear(m, m.u32(pp) + off);
    }
    srf_list_dispose(m, o, pp, 0x60, 0x64);
    srf_list_dispose(m, o, pp, 0x68, 0x6c);
    o.call(m, SI_GEN_TREE_DESTROY, &[m.u32(pp) + 0xd8, SI_BEZ_PATCH_DESTROY]);
    free_clear(m, pp);
}

/// [soft 0x084f00a0 SI_nurbsCapping], on the image (`nurbs.c`): for each capping word that
/// is set on a direction that is not closed, `SI_nurbsSrfForceCapping(srf, direction, 0)`
/// through `o`: +0x8c (V closed word +0x44) direction 0, +0x8e (+0x44) 1, +0x88 (U, +0x20) 2,
/// +0x8a (+0x20) 3.
pub fn nurbs_capping(m: &mut Mem, o: &mut dyn Outside, srf: u32) {
    for (cap, closed, dir) in [(0x8cu32, 0x44u32, 0u32), (0x8e, 0x44, 1), (0x88, 0x20, 2), (0x8a, 0x20, 3)] {
        if m.u16(srf + cap) != 0 && m.u16(srf + closed) == 0 {
            o.call(m, SI_NURBS_SRF_FORCE_CAPPING, &[srf, dir, 0]);
        }
    }
}

/// [soft 0x084e9f2c SI_nurbsSrfMovePoint], on the image (`nrb_misc.c`):
/// `SI_nurbsSrfMovePoint(srf, i, x, y, z, w)`. Point `i` of the array at +0x54 (0x24 bytes a
/// point; the index is not tested, the address wraps as the original's 32-bit arithmetic
/// does) gets the weight `w` (through the unit), bits 0x60 of its flag word's highest byte,
/// then x, y and z times `w` (each product in the register, one rounding to `double`); bits
/// 0x60 of the surface's +0x5b; for an untrimmed surface (+0x60 is 0) `markDirtyCcrvs(srf,
/// i)` through `o`; [`nurbs_capping`]; `SI_genTreeDestroy(&srf->tree, SI_bezPatchDestroy)`
/// through `o`. Returns 1.
#[allow(clippy::too_many_arguments)]
pub fn srf_move_point(m: &mut Mem, o: &mut dyn Outside, srf: u32, i: i32, x: f64, y: f64, z: f64, w: f64) -> i32 {
    let wx = si_x87::F80::from_f64(w);
    let [px, py, pz] = [x, y, z].map(|v| (si_x87::F80::from_f64(v) * wx).to_f64());
    let p = m.u32(srf + 0x54).wrapping_add((i as u32).wrapping_mul(0x24));
    m.set_f64(p.wrapping_add(0x18), wx.to_f64());
    m.or_u8(p.wrapping_add(0x23), 0x60);
    m.set_f64(p, px);
    m.set_f64(p.wrapping_add(8), py);
    m.set_f64(p.wrapping_add(0x10), pz);
    m.or_u8(srf + 0x5b, 0x60);
    if m.u32(srf + 0x60) == 0 {
        o.call(m, MARK_DIRTY_CCRVS, &[srf, i as u32]);
    }
    nurbs_capping(m, o, srf);
    o.call(m, SI_GEN_TREE_DESTROY, &[srf + 0xd8, SI_BEZ_PATCH_DESTROY]);
    1
}

/// [soft 0x084e9644 SI_nurbsSrfApplyMatrix], on the image (`nrb_misc.c`): the curve's
/// [`crv_apply_matrix`] on a surface: `SI_nurbsSrfApplyMatrix(srf, m, mask)`. The point
/// pointer (+0x54) is read once, the count (+0x48) again after each point. A point is
/// transformed when `mask` is 0 or its flag word (+0x20) has one of the mask's bits (x, y,
/// z, w read, each coordinate q stored as `((x*m[0][q] + y*m[1][q]) + z*m[2][q]) +
/// w*m[3][q]` in the register, one rounding to `double`; bits 0x60 of +0x23 set); then, for
/// an untrimmed surface (+0x60 is 0), `markDirtyCcrvs(srf, i)` through `o`. Last, bits 0x60
/// of the surface's +0x5b, whether or not a point was transformed.
pub fn srf_apply_matrix(m: &mut Mem, o: &mut dyn Outside, srf: u32, mat: u32, mask: i32) {
    use si_x87::F80;
    let mut p = m.u32(srf.wrapping_add(0x54));
    let mut i: i32 = 0;
    if 0 < m.i32(srf.wrapping_add(0x48)) {
        loop {
            if mask == 0 || (m.u32(p.wrapping_add(0x20)) & mask as u32) != 0 {
                let v: [F80; 4] = core::array::from_fn(|k| F80::from_f64(m.f64(p.wrapping_add(8 * k as u32))));
                let out: [f64; 4] = core::array::from_fn(|q| {
                    let f = |r: u32| F80::from_f32(m.f32(mat.wrapping_add(4 * (4 * r + q as u32))));
                    (((v[0] * f(0) + v[1] * f(1)) + v[2] * f(2)) + v[3] * f(3)).to_f64()
                });
                for (q, x) in out.into_iter().enumerate() {
                    m.set_f64(p.wrapping_add(8 * q as u32), x);
                }
                m.or_u8(p.wrapping_add(0x23), 0x60);
                if m.u32(srf.wrapping_add(0x60)) == 0 {
                    o.call(m, MARK_DIRTY_CCRVS, &[srf, i as u32]);
                }
            }
            i = i.wrapping_add(1);
            p = p.wrapping_add(0x24);
            if i >= m.i32(srf.wrapping_add(0x48)) {
                break;
            }
        }
    }
    m.or_u8(srf.wrapping_add(0x5b), 0x60);
}

/// [soft 0x084e9fac SI_nurbsSrfMovePointNoCap], on the image: [`srf_move_point`] without the
/// capping and without the bounding-box tree's destruction: point `i` gets the weight `w`
/// (stored first, no unit pass), bits 0x60 of its +0x23, x, y, z times `w` (each product in
/// the register, one rounding to `double`); bits 0x60 of the surface's +0x5b; for an
/// untrimmed surface `markDirtyCcrvs(srf, i)` through `o`. Returns 1.
#[allow(clippy::too_many_arguments)]
pub fn srf_move_point_no_cap(m: &mut Mem, o: &mut dyn Outside, srf: u32, i: i32, x: f64, y: f64, z: f64, w: f64) -> i32 {
    let wx = si_x87::F80::from_f64(w);
    let [px, py, pz] = [x, y, z].map(|v| (si_x87::F80::from_f64(v) * wx).to_f64());
    let p = m.u32(srf.wrapping_add(0x54)).wrapping_add((i as u32).wrapping_mul(0x24));
    m.set_f64(p.wrapping_add(0x18), w);
    m.or_u8(p.wrapping_add(0x23), 0x60);
    m.set_f64(p, px);
    m.set_f64(p.wrapping_add(8), py);
    m.set_f64(p.wrapping_add(0x10), pz);
    m.or_u8(srf.wrapping_add(0x5b), 0x60);
    if m.u32(srf.wrapping_add(0x60)) == 0 {
        o.call(m, MARK_DIRTY_CCRVS, &[srf, i as u32]);
    }
    1
}

/// [soft 0x084f0134 SI_nurbsSrfForceCapping], on the image (`nurbs.c`):
/// `SI_nurbsSrfForceCapping(srf, direction, mode)` makes one border row (direction 0: the first
/// row of `nu` points; 1: the last; 2: the first column, `nv` points `nu` apart; 3: the last)
/// collapse to one place: with `mode` 0 the place of the border's first point (x, y, z each
/// divided by the weight in the register, stored as a float), otherwise the mean of the
/// border's points (each coordinate's quotient added to the running float sum in the register
/// and stored as a float; then times `1.0/count` kept in the register, each product stored as a float). Every
/// point of the border then goes through [`srf_move_point_no_cap`] with that place and its own
/// weight. Directions outside 0 to 3 leave the original's locals unset (the capping words
/// only ever ask for 0 to 3): nothing is done here. A border of no points still computes its
/// place from the first point (mode 0) or divides by zero (mode not 0: infinity or NaN, no
/// point moved).
pub fn srf_force_capping(m: &mut Mem, o: &mut dyn Outside, srf: u32, dir: i16, mode: i16) {
    use si_x87::F80;
    let nu = m.i32(srf.wrapping_add(4));
    let nv = m.i32(srf.wrapping_add(0x28));
    let (mut idx, count, step): (i32, i32, i32) = match dir {
        0 => (0, nu, 1),
        1 => (nv.wrapping_sub(1).wrapping_mul(nu), nu, 1),
        2 => (0, nv, nu),
        3 => (nu.wrapping_sub(1), nv, nu),
        _ => return,
    };
    let [mut lx, mut ly, mut lz] = [0f32; 3];
    let pt = |m: &Mem, i: i32| m.u32(srf.wrapping_add(0x54)).wrapping_add((i as u32).wrapping_mul(0x24));
    let q = |m: &Mem, p: u32, k: u32| F80::from_f64(m.f64(p.wrapping_add(8 * k))) / F80::from_f64(m.f64(p.wrapping_add(0x18)));
    if mode == 0 {
        let p = pt(m, idx);
        lx = q(m, p, 0).to_f32();
        ly = q(m, p, 1).to_f32();
        lz = q(m, p, 2).to_f32();
    } else {
        let mut i = idx;
        let mut n = count;
        if 0 < count {
            loop {
                let p = pt(m, i);
                i = i.wrapping_add(step);
                lx = (q(m, p, 0) + F80::from_f32(lx)).to_f32();
                ly = (q(m, p, 1) + F80::from_f32(ly)).to_f32();
                lz = (q(m, p, 2) + F80::from_f32(lz)).to_f32();
                n = n.wrapping_sub(1);
                if n == 0 {
                    break;
                }
            }
        }
        // `1.0 / count` stays in the register (the listing never stores it): each product
        // is the 80-bit quotient times the float sum, one rounding to float.
        let f = F80::from_f32(1.0) / F80::from_i32(count);
        lx = (f * F80::from_f32(lx)).to_f32();
        lz = (f * F80::from_f32(lz)).to_f32();
        ly = (f * F80::from_f32(ly)).to_f32();
    }
    let mut n = count;
    if 0 < n {
        loop {
            let w = m.f64(pt(m, idx).wrapping_add(0x18));
            srf_move_point_no_cap(m, o, srf, idx, lx as f64, ly as f64, lz as f64, w);
            idx = idx.wrapping_add(step);
            n = n.wrapping_sub(1);
            if n == 0 {
                break;
            }
        }
    }
}

/// [soft 0x084ea108 SI_nurbsTransferDirKnots], on the image ([`crate::misc::transfer_dir_knots`]):
/// the source direction's knots (as many as it counts, the arrays read again at every knot)
/// and its range, through the unit.
pub fn transfer_dir_knots(m: &mut Mem, src: u32, dst: u32) {
    let mut i: i32 = 0;
    while i < m.i32(src + 8) {
        let (from, to) = (m.u32(src + 0x0c).wrapping_add(8 * i as u32), m.u32(dst + 0x0c).wrapping_add(8 * i as u32));
        mov64(m, from, to);
        i += 1;
    }
    mov64(m, src + 0x10, dst + 0x10);
    mov64(m, src + 0x18, dst + 0x18);
}

/// [soft 0x084ea07c SI_nurbsSrfTransferPointsKnots], on the image (the first form is
/// [`crate::misc::srf_transfer_points_knots`]): `SI_nurbsSrfTransferPointsKnots(src, dst)`:
/// `dst`'s display lines disposed of ([`srf_dispose_lines`]); then
/// [soft 0x084ea0c4 SI_nurbsSrfTransferPoints]: bits 0x60 of `dst`'s +0x5b and the source's
/// points (as many as its total, +0x48; nine words each) over `dst`'s; then
/// [`transfer_dir_knots`] for U and for V (+0x24).
pub fn srf_transfer_points_knots(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) {
    srf_dispose_lines(m, o, dst);
    m.or_u8(dst + 0x5b, 0x60);
    let mut i: i32 = 0;
    while i < m.i32(src + 0x48) {
        let off = 0x24u32.wrapping_mul(i as u32);
        let (from, to) = (m.u32(src + 0x54).wrapping_add(off), m.u32(dst + 0x54).wrapping_add(off));
        point_copy(m, from, to);
        i += 1;
    }
    transfer_dir_knots(m, src, dst);
    transfer_dir_knots(m, src + 0x24, dst + 0x24);
}

// ---------------------------------------------------------------------------------------
// The knots made anew from the points (`nrb_b.c`) and the conversion of a curve to straight
// pieces (`nrb_convert.c`), on the image: what a face of NURBS curves is made linear with
// before it is triangulated (`SI_faceConvertToLinear`, finding 0012 section 18.5).
// ---------------------------------------------------------------------------------------

/// The shortest chord `si_nurbsCrvFillDistancePoints` keeps (0x08661750): the double
/// nearest 0.01.
const MIN_CHORD: f64 = 0.01;

/// The half of `knotsCrvReparamChordCentriQuad` (0x08661760).
const HALF: f64 = 0.5;

/// A control point divided by its weight, each coordinate rounded to `float` (the
/// `fdiv`, `fmul`, `fstp DWORD` of `si_nurbsCrvFillDistancePoints`; 1 / w stays in the
/// register).
fn point_f32(m: &Mem, p: u32) -> [f32; 3] {
    use si_x87::x;
    let inv = si_x87::F80::ONE / x(m.f64(p.wrapping_add(0x18)));
    core::array::from_fn(|k| (inv * x(m.f64(p.wrapping_add(8 * k as u32)))).to_f32())
}

/// The chord between two points as the original sums it: (dx² + dy²) + dz² in the
/// register, its root (twice for a curve parametrised by 4, the centripetal one), at least
/// [`MIN_CHORD`] (a NaN is kept).
fn chord(a: [f32; 3], b: [f32; 3], quarter: bool) -> si_x87::F80 {
    use si_x87::x;
    let d: [si_x87::F80; 3] = core::array::from_fn(|k| x(a[k]) - x(b[k]));
    let mut r = ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt();
    if quarter {
        r = r.sqrt();
    }
    let min = x(MIN_CHORD);
    if r.lt(min) {
        min
    } else {
        r
    }
}

/// [soft 0x084ddfdc si_nurbsCrvFillDistancePoints]: `(curve, out, &total, closed)`. For
/// each pair of neighbouring points the chord ([`chord`], the points divided by their
/// weights and rounded to `float`) into `out[i]`, the sum kept as a `double` in `*total`;
/// with `closed` not 0 also the chord from the last point back to the first into
/// `out[n − 1]` (not summed). The point count and the point pointer are read again at every
/// pair. The closing chord of a curve of one point reads floats the original never wrote:
/// refused.
pub fn crv_fill_distance_points(m: &mut Mem, curve: u32, out: u32, total: u32, closed: i16) -> Result<(), Halt> {
    use si_x87::x;
    let mut sum = 0.0f64;
    let mut last: Option<[f32; 3]> = None;
    let mut i: i32 = 0;
    while i < m.i32(curve + 4).wrapping_sub(1) {
        let a = m.u32(curve + 0x2c).wrapping_add(0x24u32.wrapping_mul(i as u32));
        let quarter = m.i32(curve + 0x24) == 4;
        let (pa, pb) = (point_f32(m, a), point_f32(m, a.wrapping_add(0x24)));
        let d = chord(pa, pb, quarter);
        sum = (x(sum) + d).to_f64();
        m.set_f64(out.wrapping_add(8 * i as u32), d.to_f64());
        last = Some(pb);
        i += 1;
    }
    if closed != 0 {
        let Some(l) = last else { return Err(Halt::Refused("si_nurbsCrvFillDistancePoints")) };
        let p0 = point_f32(m, m.u32(curve + 0x2c));
        let d = chord(p0, l, m.i32(curve + 0x24) == 4);
        m.set_f64(out.wrapping_add(8 * i as u32), d.to_f64());
    }
    m.set_f64(total, sum);
    Ok(())
}

pub(crate) fn at8(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32).wrapping_mul(8))
}

/// `a[i − 1] + b[j]` in the register, stored as a `double` at `a[i]`.
pub(crate) fn add_into(m: &mut Mem, a: u32, i: i32, b: u32, j: i32) {
    use si_x87::x;
    let v = x(m.f64(at8(a, i - 1))) + x(m.f64(at8(b, j)));
    m.set_f64(at8(a, i), v.to_f64());
}

/// The knot count of a direction made anew: n + 1 closed, order + n − 2 open.
pub(crate) fn knot_count(m: &Mem, dir: u32) -> i32 {
    let n = m.i32(dir + 4);
    if m.i16(dir + 0x20) != 0 {
        n.wrapping_add(1)
    } else {
        m.i32(dir).wrapping_add(n).wrapping_sub(2)
    }
}

/// [soft 0x084de2e0 knotsCrvReparamChordCentri]: `(dir, d)`, the knots of a direction from
/// the chords `d`: the count ([`knot_count`]) into +8, `si_knotsAllocate` into +0xc. Closed
/// or of order 2: knot 0 is 0 and each next one the one before plus the next chord. Open:
/// the first order − 2 chords summed in the register; knots 0 to order − 2 are 0, knot
/// order − 1 that sum, the next ones up to count − order each the one before plus the next
/// chord; then (more points than the order) the sum begun again from 0 with the chords
/// left, and every knot after count − order is knot count − order plus the sum.
pub fn knots_crv_reparam_chord_centri(m: &mut Mem, dir: u32, d: u32) {
    use si_x87::{x, F80};
    let n = m.i32(dir + 4);
    let order = m.i32(dir);
    let nk = knot_count(m, dir);
    m.set_i32(dir + 8, nk);
    let k = knots_allocate(m, nk);
    m.set_u32(dir + 0xc, k);
    if m.i16(dir + 0x20) != 0 || order == 2 {
        let mut e = 1;
        while e < nk {
            add_into(m, k, e, d, e - 1);
            e += 1;
        }
    } else {
        let mut sum = F80::ZERO;
        let mut b = 0;
        for _ in 0..order.wrapping_sub(2).max(0) {
            sum = sum + x(m.f64(at8(d, b)));
            b += 1;
        }
        let mut e = 0;
        while e < order.wrapping_sub(1) {
            m.set_u64(at8(k, e), 0);
            e += 1;
        }
        m.set_f64(at8(k, e), sum.to_f64());
        e += 1;
        let local = nk.wrapping_sub(order);
        let end = nk.wrapping_sub(order.wrapping_sub(1));
        while e < end {
            add_into(m, k, e, d, b);
            b += 1;
            e += 1;
        }
        if n > order {
            sum = F80::ZERO;
            let end2 = local.wrapping_add(2);
            while e <= end2 {
                e += 1;
                sum = sum + x(m.f64(at8(d, b)));
                b += 1;
            }
        }
        let v = sum + x(m.f64(at8(k, local)));
        let mut e = local.wrapping_add(1);
        while e < nk {
            m.set_f64(at8(k, e), v.to_f64());
            e += 1;
        }
    }
    m.set_u32(dir + 0xc, k);
}

/// [soft 0x084de430 knotsCrvReparamChordCentriQuad]: `(dir, d)`, the knots of a direction of
/// order 3 from the chords `d`: their running sums into a scratch array of
/// n + closed − 1 (`si_knotsAllocate`; the first copied, each next the one before plus the
/// chord), the count into +8, `si_knotsAllocate` into +0xc. Closed or of order 2: knot 0 is
/// 0, each next up to count − 2 is (d[i − 1] + d[i]) × 0.5 plus the one before, the last
/// the running sum before it. Open: knots 0 to order − 2 are 0, the next ones up to count −
/// order the mean of two running sums (s[i − 2] + s[i − 1]) × 0.5, the rest the last running
/// sum. The scratch array is freed.
pub fn knots_crv_reparam_chord_centri_quad(m: &mut Mem, dir: u32, d: u32) {
    use si_x87::x;
    let order = m.i32(dir);
    let cnt = m.i32(dir + 4).wrapping_add(m.i16(dir + 0x20) as i32).wrapping_sub(1);
    let s = knots_allocate(m, cnt);
    mov64(m, d, s);
    let mut e = 1;
    while e < cnt {
        add_into(m, s, e, d, e);
        e += 1;
    }
    let nk = knot_count(m, dir);
    m.set_i32(dir + 8, nk);
    let k = knots_allocate(m, nk);
    m.set_u32(dir + 0xc, k);
    let half = x(HALF);
    if m.i16(dir + 0x20) != 0 || order == 2 {
        m.set_u64(k, 0);
        let mut e = 1;
        while e < nk.wrapping_sub(1) {
            let v = (x(m.f64(at8(d, e - 1))) + x(m.f64(at8(d, e)))) * half + x(m.f64(at8(k, e - 1)));
            m.set_f64(at8(k, e), v.to_f64());
            e += 1;
        }
        mov64(m, at8(s, e - 1), at8(k, e));
    } else {
        let mut e = 0;
        while e < order.wrapping_sub(1) {
            m.set_u64(at8(k, e), 0);
            e += 1;
        }
        let mut e = order.wrapping_sub(1);
        let end = nk.wrapping_sub(order.wrapping_sub(1));
        while e < end {
            let v = (x(m.f64(at8(s, e - 2))) + x(m.f64(at8(s, e - 1)))) * half;
            m.set_f64(at8(k, e), v.to_f64());
            e += 1;
        }
        let mut e = nk.wrapping_sub(order).wrapping_add(1);
        while e < nk {
            mov64(m, at8(s, cnt - 1), at8(k, e));
            e += 1;
        }
    }
    m.set_u32(dir + 0xc, k);
    free_some(m, s);
}

/// The chords of a curve made into its knots (the part `SI_knotsCrvReparametrization` does
/// twice): `si_knotsAllocate(n + closed − 1)`, [`crv_fill_distance_points`] (the total to a
/// local), for a closed curve the chords turned one place to the right (the last first),
/// [`knots_crv_reparam_chord_centri_quad`] for order 3, else
/// [`knots_crv_reparam_chord_centri`]; the chords freed.
fn knots_from_chords(m: &mut Mem, c: u32) -> Result<(), Halt> {
    let closed = m.i16(c + 0x20);
    let d = knots_allocate(m, m.i32(c + 4).wrapping_add(closed as i32).wrapping_sub(1));
    let total = m.local(8);
    m.set_u64(total, 0);
    let r = crv_fill_distance_points(m, c, d, total, closed);
    m.release(8);
    r?;
    if m.i16(c + 0x20) != 0 {
        let n = m.i32(c + 4);
        let last = m.f64(at8(d, n - 1));
        let mut i = n - 1;
        while i > 0 {
            mov64(m, at8(d, i - 1), at8(d, i));
            i -= 1;
        }
        m.set_f64(d, si_x87::fmov64(last));
    }
    if m.i32(c) == 3 {
        knots_crv_reparam_chord_centri_quad(m, c, d);
    } else {
        knots_crv_reparam_chord_centri(m, c, d);
    }
    free_some(m, d);
    Ok(())
}

/// The range of a curve from its knots: closed, the first and knot `count − 1`; open, knot
/// order − 2 and knot count − order + 1.
pub(crate) fn range_from_knots(m: &mut Mem, c: u32, count: i32) {
    let k = m.u32(c + 0xc);
    let order = m.i32(c);
    if m.i16(c + 0x20) != 0 {
        mov64(m, k, c + 0x10);
        mov64(m, at8(k, count.wrapping_sub(1)), c + 0x18);
    } else {
        mov64(m, at8(k, order.wrapping_sub(2)), c + 0x10);
        mov64(m, at8(k, count.wrapping_sub(order).wrapping_add(1)), c + 0x18);
    }
}

/// [soft 0x084de590 SI_knotsCrvReparametrization], on the image: the knots of a curve made
/// anew from its parametrisation (+0x24). Nothing for fewer points than the order.
///
/// - 2: only a curve without knots gets them from its chords ([`knots_from_chords`]); then
///   when |knot 0| is above 1e-6 every knot less knot 0 (the difference in the register:
///   stored, and 0 when its magnitude is below 1e-6); a knot less than 1e-6 above the one
///   before is set to it; the range ([`range_from_knots`] with +8).
/// - 0 (uniform): a curve without knots gets the count ([`knot_count`]) and
///   `si_knotsAllocate`; with z = order − (closed × (order − 2) + 1) the first z knots are 0,
///   the ones from z to count − z the integers 1, 2, …, and an open curve's last order − 2
///   repeat the one before them; then the range.
/// - any other: the knots freed, made from the chords, the range.
pub fn knots_crv_reparametrization(m: &mut Mem, c: u32) -> Result<(), Halt> {
    use si_x87::x;
    let order = m.i32(c);
    if m.i32(c + 4) < order {
        return Ok(());
    }
    let count;
    match m.i32(c + 0x24) {
        2 => {
            if m.u32(c + 0xc) == 0 {
                knots_from_chords(m, c)?;
            }
            let k = m.u32(c + 0xc);
            let k0 = x(m.f64(k));
            let eps = x(crate::EPS);
            if eps.lt(k0.abs()) {
                let mut i = 0;
                while i < m.i32(c + 8) {
                    let v = x(m.f64(at8(k, i))) - k0;
                    m.set_f64(at8(k, i), v.to_f64());
                    if v.abs().lt(eps) {
                        m.set_u64(at8(k, i), 0);
                    }
                    i += 1;
                }
            }
            let mut i = 0;
            while i < m.i32(c + 8).wrapping_sub(1) {
                let a = x(m.f64(at8(k, i)));
                if (x(m.f64(at8(k, i + 1))) - a).lt(eps) {
                    m.set_f64(at8(k, i + 1), a.to_f64());
                }
                i += 1;
            }
            count = m.i32(c + 8);
        }
        0 => {
            if m.u32(c + 0xc) == 0 {
                let nk = knot_count(m, c);
                m.set_i32(c + 8, nk);
                let k = knots_allocate(m, nk);
                m.set_u32(c + 0xc, k);
                let lead = order.wrapping_sub(2);
                let zeros = order.wrapping_sub((m.i16(c + 0x20) as i32).wrapping_mul(lead).wrapping_add(1));
                let mut i = 0;
                while i < zeros {
                    m.set_u64(at8(k, i), 0);
                    i += 1;
                }
                let mut j: i32 = 1;
                let mut i = zeros;
                while i <= nk.wrapping_sub(zeros) {
                    m.set_f64(at8(k, i), j as f64);
                    j = j.wrapping_add(1);
                    i += 1;
                }
                if m.i16(c + 0x20) == 0 {
                    for t in 0..lead.max(0) {
                        let to = nk.wrapping_sub(lead).wrapping_add(t);
                        mov64(m, at8(k, to - 1), at8(k, to));
                    }
                }
                count = nk;
            } else {
                count = m.i32(c + 8);
            }
        }
        _ => {
            free_clear(m, c + 0xc);
            knots_from_chords(m, c)?;
            count = m.i32(c + 8);
        }
    }
    range_from_knots(m, c, count);
    Ok(())
}

/// [soft 0x084e0eb4 SI_nurbsCrvConvertToLinear], on the image: a curve of order 2 through
/// points of the curve at `c`. Null for a null curve, one without points or without knots;
/// a copy ([`crv_copy`]) for one of order 2 already. Otherwise the curve is sampled
/// (`SI_SampleKnotVector` with its step, +0x34); a new curve (`SI_nurbsCrvAllocate`) of
/// order 2, the sample count less one if closed points, the closed word, step 1, the points
/// (`SI_nurbsPointsAllocate`) each the curve's value at a sample ([`crv_value`]; x, y, z;
/// the weight left at 1, the flag 0); the samples freed; the knots made
/// ([`knots_crv_reparametrization`]: uniform, the new curve's +0x24 being 0).
pub fn crv_convert_to_linear(m: &mut Mem, c: u32) -> Result<u32, Halt> {
    if c == 0 || m.i32(c + 4) <= 0 || m.u32(c + 0xc) == 0 {
        return Ok(0);
    }
    if m.i32(c) == 2 {
        return Ok(crv_copy(m, c));
    }
    let (array, count) = sampled(m, c, m.i32(c + 0x34))?.unwrap_or((0, 0));
    let r = nurbs_crv_allocate(m).addr();
    if r != 0 {
        m.set_i32(r, 2);
        let n = if m.i16(c + 0x20) != 0 { count.wrapping_sub(1) } else { count };
        m.set_i32(r + 4, n);
        m.set_i16(r + 0x20, m.i16(c + 0x20));
        m.set_i32(r + 0x34, 1);
        let pts = nurbs_points_allocate(m, n);
        m.set_u32(r + 0x2c, pts);
        let mut i = 0;
        while i < m.i32(r + 4) {
            let v = crv_value(m, c, m.f64(at8(array, i)), 0)?;
            let p = m.u32(r + 0x2c).wrapping_add(0x24u32.wrapping_mul(i as u32));
            for (k, w) in v.iter().enumerate() {
                m.set_f64(p + 8 * k as u32, si_x87::fmov64(*w));
            }
            m.set_u32(p + 0x20, 0);
            i += 1;
        }
    }
    free_some(m, array);
    if r == 0 {
        return Err(Halt::Refused("SI_nurbsCrvConvertToLinear"));
    }
    knots_crv_reparametrization(m, r)?;
    Ok(r)
}

// ---------------------------------------------------------------------------------------
// The reversal of a curve (`nrb_b.c`), on the image: what `SI_faceOrdinatePoints` turns a
// contour of a face of NURBS curves round with (`SI_faceConvertToMesh`, finding 0012,
// section 18.5).
// ---------------------------------------------------------------------------------------

/// `SI_gblIndex` (0x0878896c): when not null, the table of integers in which the editing
/// routines write where each point went.
pub const SI_GBL_INDEX: u32 = 0x0878_896c;
/// The animation package's renumbering of a path curve's keys.
pub const SI_INVERT_FCURVE_PATH_KEYS: Routine = Routine::new("SI_invertFcurvePathKeys", 0x0821_fd10);

/// [soft 0x084dc954 si_nurbsDirInverse], on the image (the first form is
/// [`crate::knots::dir_inverse`]): `si_knotsAllocate(count)` for the intervals, each
/// knot[i + 1] − knot[i] (`fld`, `fsub`, `fstp` double) for i below count − 1; then from
/// `first` (2 for a closed direction whose order is not 3, else 1) each knot[i] = knot[i − 1]
/// + interval[count − i + first − 2] (`fld`, `fadd`, `fstp`); the count and the knot pointer
/// are read again at every step. The range from the knots (closed: the first and knot
/// count − 1; open: knot order − 2 and knot count − order + 1, through the unit); the
/// intervals freed. Answers 1.
pub fn dir_inverse(m: &mut Mem, dir: u32) -> i32 {
    use si_x87::x;
    let d = knots_allocate(m, m.i32(dir + 8));
    let mut i: i32 = 0;
    while i < m.i32(dir + 8).wrapping_sub(1) {
        let k = m.u32(dir + 0xc);
        let v = x(m.f64(at8(k, i.wrapping_add(1)))) - x(m.f64(at8(k, i)));
        m.set_f64(at8(d, i), v.to_f64());
        i = i.wrapping_add(1);
    }
    let first = if m.i16(dir + 0x20) == 0 || m.i32(dir) == 3 { 1 } else { 2 };
    let mut i = first;
    loop {
        let n = m.i32(dir + 8);
        if i >= n {
            break;
        }
        let k = m.u32(dir + 0xc);
        let j = n.wrapping_sub(i).wrapping_add(first);
        let v = x(m.f64(at8(k, i.wrapping_sub(1)))) + x(m.f64(at8(d, j.wrapping_sub(2))));
        m.set_f64(at8(k, i), v.to_f64());
        i = i.wrapping_add(1);
    }
    let s = if m.i16(dir + 0x20) == 0 { m.i32(dir).wrapping_sub(2) } else { 0 };
    mov64(m, at8(m.u32(dir + 0xc), s), dir + 0x10);
    let e = if m.i16(dir + 0x20) != 0 { m.i32(dir + 8).wrapping_sub(1) } else { m.i32(dir + 8).wrapping_sub(m.i32(dir)).wrapping_add(1) };
    mov64(m, at8(m.u32(dir + 0xc), e), dir + 0x18);
    free_some(m, d);
    1
}

/// [soft 0x084dca24 SI_nurbsCrvInverse], on the image (the first form is
/// [`crate::edit::crv_inverse`]): the points exchanged end for end, nine words each through
/// a copy on the stack (the point pointer read again for each of the three moves); where
/// `SI_gblIndex` is set, table[j] = i and table[i] = j for each pair (the table's pointer
/// read again for the second); then [`dir_inverse`]; for a curve not null whose word at
/// +0x4c is not 0, `SI_findNURBSPathFcurveOwner(curve)` and
/// `SI_invertFcurvePathKeys(owner, 0, curve)` through `o`. Answers 1.
pub fn crv_inverse(m: &mut Mem, o: &mut dyn Outside, c: u32) -> i32 {
    let mut i: i32 = 0;
    let mut j = m.i32(c + 4).wrapping_sub(1);
    if i < j {
        loop {
            let (ai, aj) = (0x24u32.wrapping_mul(i as u32), 0x24u32.wrapping_mul(j as u32));
            let p = m.u32(c + 0x2c);
            let held: [u32; 9] = core::array::from_fn(|k| m.u32(p.wrapping_add(aj).wrapping_add(4 * k as u32)));
            let p = m.u32(c + 0x2c);
            point_copy(m, p.wrapping_add(ai), p.wrapping_add(aj));
            let p = m.u32(c + 0x2c);
            for (k, w) in held.iter().enumerate() {
                m.set_u32(p.wrapping_add(ai).wrapping_add(4 * k as u32), *w);
            }
            let t = m.u32(SI_GBL_INDEX);
            if t != 0 {
                m.set_i32(t.wrapping_add(4u32.wrapping_mul(j as u32)), i);
                let t = m.u32(SI_GBL_INDEX);
                m.set_i32(t.wrapping_add(4u32.wrapping_mul(i as u32)), j);
            }
            i = i.wrapping_add(1);
            j = j.wrapping_sub(1);
            if i >= j {
                break;
            }
        }
    }
    dir_inverse(m, c);
    if c != 0 && m.i16(c + 0x4c) != 0 {
        let owner = o.call(m, SI_FIND_NURBS_PATH_FCURVE_OWNER, &[c]).eax;
        o.call(m, SI_INVERT_FCURVE_PATH_KEYS, &[owner, 0, c]);
    }
    1
}

// ---------------------------------------------------------------------------------------
// `nrb_point.c`: deleting and adding points, on the image (2026-10-28).
// ---------------------------------------------------------------------------------------

/// The animation package's removal of one key of a path curve's fcurve.
pub const SI_DELETE_FCURVE_PATH_KEY: Routine = Routine::new("SI_deleteFcurvePathKey", 0x0821_ff50);

/// [soft 0x084f0328 SI_crvDeleteTagPoints], on the image (`nrb_point.c`):
/// `SI_crvDeleteTagPoints(curve)` removes the control points whose flag word (+0x20) has
/// bit 0 set. For a curve whose +0x4c word is 1 the path owner is asked first
/// (`SI_findNURBSPathFcurveOwner`). Each point is visited in order (the point pointer and
/// the count read again at every step): a flagged one adds to the count of deleted points,
/// has its key deleted at `i − deleted − 1` (a signed 16-bit argument, through `o`) and
/// `SI_gblIndex[i]` set to −1 where that table exists; an unflagged one is copied (nine
/// words) down to the next free slot, `SI_gblIndex[i]` set to the slot number. When any was
/// deleted the count is reduced; below the order (+0) the knot count (+8) is cleared; the
/// knot block (+0xc) is freed and its pointer cleared; the knots are made anew
/// ([`knots_crv_reparametrization`]). Answers whether any point was deleted.
pub fn crv_delete_tag_points(m: &mut Mem, o: &mut dyn Outside, c: u32) -> Result<i32, Halt> {
    let mut deleted: i32 = 0;
    let mut owner: u32 = 0;
    if m.i16(c.wrapping_add(0x4c)) == 1 {
        owner = o.call(m, SI_FIND_NURBS_PATH_FCURVE_OWNER, &[c]).eax;
    }
    let mut kept: i32 = 0;
    let mut i: i32 = 0;
    let (mut src, mut dst): (u32, u32) = (0, 0);
    let mut count = m.i32(c.wrapping_add(4));
    if 0 < count {
        loop {
            let p = m.u32(c.wrapping_add(0x2c));
            if m.u8(p.wrapping_add(src).wrapping_add(0x20)) & 1 == 0 {
                point_copy(m, p.wrapping_add(src), p.wrapping_add(dst));
                let t = m.u32(SI_GBL_INDEX);
                if t != 0 {
                    m.set_i32(t.wrapping_add(4u32.wrapping_mul(i as u32)), kept);
                }
                dst = dst.wrapping_add(0x24);
                kept = kept.wrapping_add(1);
            } else {
                if owner != 0 {
                    let key = i.wrapping_sub(deleted).wrapping_sub(1) as i16 as i32 as u32;
                    o.call(m, SI_DELETE_FCURVE_PATH_KEY, &[owner, key]);
                }
                let t = m.u32(SI_GBL_INDEX);
                if t != 0 {
                    m.set_i32(t.wrapping_add(4u32.wrapping_mul(i as u32)), -1);
                }
                deleted = deleted.wrapping_add(1);
            }
            src = src.wrapping_add(0x24);
            i = i.wrapping_add(1);
            count = m.i32(c.wrapping_add(4));
            if i >= count {
                break;
            }
        }
    }
    if deleted != 0 {
        let n = count.wrapping_sub(deleted);
        m.set_i32(c.wrapping_add(4), n);
        if n < m.i32(c) {
            m.set_i32(c.wrapping_add(8), 0);
        }
        free_clear(m, c.wrapping_add(0xc));
        knots_crv_reparametrization(m, c)?;
    }
    Ok((deleted != 0) as i32)
}

/// [soft 0x084ef184 nurbsDirDeleteKnot], on the image (`nrb_point.c`, static):
/// `nurbsDirDeleteKnot(dir, index)` makes a new knot array one shorter (`si_calloc(count − 1,
/// 8)`; the count at +8 is already reduced when it is allocated) without one knot, and answers
/// it (the direction's own pointer is not touched). The knot dropped is the one after `index`
/// (index + 1), clamped: an open direction (+0x20 zero) to order − 1 … count − order, a
/// closed one to 0 … count − 1. The knots below are copied (`fld`, `fstp` through the unit),
/// the rest from one place up; the count is read again at every step of the second loop.
pub fn dir_delete_knot(m: &mut Mem, dir: u32, index: i32) -> u32 {
    let mut b = index.wrapping_add(1);
    let count = m.i32(dir + 8);
    let order = m.i32(dir);
    let hi;
    if m.i16(dir + 0x20) == 0 {
        let lo = order.wrapping_sub(1);
        if b < lo {
            b = lo;
            hi = None;
        } else {
            hi = Some(count.wrapping_sub(order));
        }
    } else if b < 0 {
        b = 0;
        hi = None;
    } else {
        hi = Some(count.wrapping_sub(1));
    }
    if let Some(h) = hi {
        if b > h {
            b = h;
        }
    }
    let src = m.u32(dir + 0xc);
    let n = m.i32(dir + 8);
    m.set_i32(dir + 8, n.wrapping_sub(1));
    let new = knots_allocate(m, n.wrapping_sub(1));
    let mut i: i32 = 0;
    while i < b {
        mov64(m, at8(src, i), at8(new, i));
        i = i.wrapping_add(1);
    }
    let mut i = b;
    while i < m.i32(dir + 8) {
        mov64(m, at8(src, i.wrapping_add(1)), at8(new, i));
        i = i.wrapping_add(1);
    }
    new
}

/// [soft 0x084ef214 nurbsCrvDeleteKnotNonUniform], on the image (`nrb_point.c`, static): when
/// the curve has at least `order` points (+4 against +0), and its parameterisation word
/// (+0x24) is 2 with the order not 2, the knots lose one ([`dir_delete_knot`]; the old block
/// freed, the new one put in); otherwise the knot block is freed and its pointer cleared
/// (the knots are made anew afterwards).
pub fn crv_delete_knot_non_uniform(m: &mut Mem, c: u32, index: i32) {
    let order = m.i32(c);
    if m.i32(c + 4) < order {
        return;
    }
    if m.i32(c + 0x24) == 2 && order != 2 {
        let new = dir_delete_knot(m, c, index);
        free_clear(m, c + 0xc);
        m.set_u32(c + 0xc, new);
    } else {
        free_clear(m, c + 0xc);
    }
}

/// [soft 0x084ef06c SI_nurbsCrvDeletePoint], on the image (`nrb_point.c`):
/// `SI_nurbsCrvDeletePoint(curve, index)`. Nothing is done for an index below 0 that is also
/// not below the point count (the original's test is `0 <= i || i < count`, so any negative
/// index passes it only when it is below the count: always). A new point block
/// (`SI_nurbsPointsAllocate(count − 1)`) is made; for a curve whose +0x4c word is 1 the path
/// owner is asked (`SI_findNURBSPathFcurveOwner`) and `SI_deleteFcurvePathKey(owner,
/// (short)(index − 1))` called through `o`; the count is reduced; `SI_gblIndex[index]` is −1
/// where the table exists; points below the index are copied, those above moved down one
/// (with `SI_gblIndex[j + 1] = j`); the old block freed, the new one put in; then the knots
/// ([`crv_delete_knot_non_uniform`], [`knots_crv_reparametrization`]).
pub fn crv_delete_point(m: &mut Mem, o: &mut dyn Outside, c: u32, index: i32) -> Result<(), Halt> {
    if !(index >= 0 || index < m.i32(c + 4)) {
        return Ok(());
    }
    let new = nurbs_points_allocate(m, m.i32(c + 4).wrapping_sub(1));
    if m.i16(c + 0x4c) == 1 {
        let key = index.wrapping_sub(1) as i16 as i32 as u32;
        let owner = o.call(m, SI_FIND_NURBS_PATH_FCURVE_OWNER, &[c]).eax;
        o.call(m, SI_DELETE_FCURVE_PATH_KEY, &[owner, key]);
    }
    let t = m.u32(SI_GBL_INDEX);
    m.set_i32(c + 4, m.i32(c + 4).wrapping_sub(1));
    if t != 0 {
        m.set_i32(t.wrapping_add(4u32.wrapping_mul(index as u32)), -1);
    }
    let mut off: u32 = 0;
    let mut k = index;
    if k > 0 {
        loop {
            point_copy(m, m.u32(c + 0x2c).wrapping_add(off), new.wrapping_add(off));
            off = off.wrapping_add(0x24);
            k = k.wrapping_sub(1);
            if k == 0 {
                break;
            }
        }
    }
    let mut j = index;
    if j < m.i32(c + 4) {
        let mut off = 0x24u32.wrapping_mul(index as u32);
        loop {
            point_copy(m, m.u32(c + 0x2c).wrapping_add(off).wrapping_add(0x24), new.wrapping_add(off));
            let t = m.u32(SI_GBL_INDEX);
            if t != 0 {
                m.set_i32(t.wrapping_add(4u32.wrapping_mul(j as u32)).wrapping_add(4), j);
            }
            off = off.wrapping_add(0x24);
            j = j.wrapping_add(1);
            if j >= m.i32(c + 4) {
                break;
            }
        }
    }
    free_clear(m, c + 0x2c);
    m.set_u32(c + 0x2c, new);
    crv_delete_knot_non_uniform(m, c, index);
    knots_crv_reparametrization(m, c)
}

/// `SI_knotsSrfReparametrization (0x084ded40)`, `nrb_b.c`: not ported; goes through `o`.
pub const SI_KNOTS_SRF_REPARAMETRIZATION: Routine = Routine::new("SI_knotsSrfReparametrization", 0x084d_ed40);

/// [soft 0x084ef288 SI_nurbsSrfDeleteRow] on the image (`nrb_point.c`):
/// `SI_nurbsSrfDeleteRow(srf, index, dir)`. `dir` 0 deletes the row `index / nu` (V
/// direction, record +0x24, `nv` at +0x28), any other value the column `index % nu` (U
/// direction, record +0, `nu` at +4); nothing is done (answer 0) when the direction has no
/// more points than its order. The point total (+0x48) is set to the new count, a new point
/// block made (`SI_nurbsPointsAllocate`), the points of the other rows or columns copied in
/// order (nine words each; the source pointer read once), `SI_gblIndex[old] = new` for
/// those kept and −1 for those dropped; the direction's point count reduced; its knots
/// made anew without one ([`dir_delete_knot`]; the old block freed); `SI_knotsSrfReparametrization`
/// through `o`; the old points freed, the new block put in. Answers 1. `idiv` by a zero `nu`
/// (or `i32::MIN / −1`) traps in the original: refused.
pub fn srf_delete_row(m: &mut Mem, o: &mut dyn Outside, s: u32, index: i32, dir: i32) -> Result<i32, Halt> {
    let (cnt_off, knots_off, dir_rec) = if dir == 0 { (0x28u32, 0x30u32, s + 0x24) } else { (4, 0x0c, s) };
    if dir == 0 {
        if m.i32(s + 0x28) <= m.i32(s + 0x24) {
            return Ok(0);
        }
    } else if m.i32(s + 4) <= m.i32(s) {
        return Ok(0);
    }
    let nu = m.i32(s + 4);
    if nu == 0 || (nu == -1 && index == i32::MIN) {
        return Err(Halt::Refused("SI_nurbsSrfDeleteRow: division by zero"));
    }
    let (quo, rem) = (index.wrapping_div(nu), index.wrapping_rem(nu));
    let total = if dir == 0 { m.i32(s + 0x28).wrapping_sub(1).wrapping_mul(nu) } else { nu.wrapping_sub(1).wrapping_mul(m.i32(s + 0x28)) };
    m.set_i32(s + 0x48, total);
    let new = nurbs_points_allocate(m, total);
    let mut dst = new;
    let mut src = m.u32(s + 0x54);
    let (mut kept, mut seq): (i32, i32) = (0, 0);
    let mut row: i32 = 0;
    if row < m.i32(s + 0x28) {
        loop {
            let next = row.wrapping_add(1);
            let mut col: i32 = 0;
            if col < m.i32(s + 4) {
                loop {
                    let drop = if dir == 0 { row == quo } else { col == rem };
                    if !drop {
                        point_copy(m, src, dst);
                        dst = dst.wrapping_add(0x24);
                        let t = m.u32(SI_GBL_INDEX);
                        if t != 0 {
                            m.set_i32(t.wrapping_add(4u32.wrapping_mul(seq as u32)), kept);
                        }
                        kept = kept.wrapping_add(1);
                    } else {
                        let t = m.u32(SI_GBL_INDEX);
                        if t != 0 {
                            m.set_i32(t.wrapping_add(4u32.wrapping_mul(seq as u32)), -1);
                        }
                    }
                    col = col.wrapping_add(1);
                    src = src.wrapping_add(0x24);
                    seq = seq.wrapping_add(1);
                    if col >= m.i32(s + 4) {
                        break;
                    }
                }
            }
            row = next;
            if row >= m.i32(s + 0x28) {
                break;
            }
        }
    }
    m.set_i32(s + cnt_off, m.i32(s + cnt_off).wrapping_sub(1));
    let knots = dir_delete_knot(m, dir_rec, if dir == 0 { quo } else { rem });
    free_clear(m, s + knots_off);
    m.set_u32(s + knots_off, knots);
    o.call(m, SI_KNOTS_SRF_REPARAMETRIZATION, &[s]);
    free_clear(m, s + 0x54);
    m.set_u32(s + 0x54, new);
    Ok(1)
}

/// `nurbsCrvAddKnotNonUniform (0x084ee4a8)`, `nrb_point.c`, static: not ported; goes through
/// `o` with (curve, index, the parameter as a double's two words).
pub const NURBS_CRV_ADD_KNOT_NON_UNIFORM: Routine = Routine::new("nurbsCrvAddKnotNonUniform", 0x084e_e4a8);
/// The animation package's insertion of one key into a path curve's fcurve.
pub const SI_ADD_FCURVE_PATH_KEY: Routine = Routine::new("SI_addFcurvePathKey", 0x0822_0008);

/// [soft 0x084eeacc SI_nurbsCrvAddPoint], on the image (`nrb_point.c`):
/// `SI_nurbsCrvAddPoint(curve, index, t, x, y, z, w)` puts one control point into a curve.
/// Without a point block (+0x2c null) one point is allocated (`SI_nurbsPointsAllocate(1)`)
/// and `index` becomes 0; otherwise the block is grown to count + 1 points (`SI_realloc`).
/// The index is clamped to 0 … count. The points from the last down to `index` move up one
/// place (nine words each; `SI_gblIndex[j] = j + 1` where the table exists, read again at every
/// step). The new point is (w·x, w·y, w·z, w) — each product rounded once from the register —
/// with flag 0, the count grows, `nurbsCrvAddKnotNonUniform (0x084ee4a8)` (not ported) is called
/// through `o`, the knots are made anew ([`knots_crv_reparametrization`]) and, for a curve whose
/// +0x4c word is 1, `SI_addFcurvePathKey(owner, index − 1)` is called with the path owner.
pub fn crv_add_point(m: &mut Mem, o: &mut dyn Outside, c: u32, index: i32, t: f64, x: f64, y: f64, z: f64, w: f64) -> Result<(), Halt> {
    use si_x87::x as ext;
    let mut idx = index;
    let p = m.u32(c + 0x2c);
    if p != 0 {
        let n = m.i32(c + 4).wrapping_add(1).wrapping_mul(0x24) as u32;
        let np = m.si_realloc(p, n);
        m.set_u32(c + 0x2c, np);
    } else {
        let np = nurbs_points_allocate(m, 1);
        m.set_u32(c + 0x2c, np);
        idx = 0;
    }
    let count = m.i32(c + 4);
    if idx > count {
        idx = count;
    }
    if idx < 0 {
        idx = 0;
    }
    let px = (ext(w) * ext(x)).to_f64();
    let py = (ext(w) * ext(y)).to_f64();
    let pz = (ext(w) * ext(z)).to_f64();
    let mut j = count.wrapping_sub(1);
    while j >= idx {
        let from = m.u32(c + 0x2c).wrapping_add(0x24u32.wrapping_mul(j as u32));
        point_copy(m, from, from.wrapping_add(0x24));
        let tb = m.u32(SI_GBL_INDEX);
        if tb != 0 {
            m.set_i32(tb.wrapping_add(4u32.wrapping_mul(j as u32)), j.wrapping_add(1));
        }
        j = j.wrapping_sub(1);
    }
    let at = m.u32(c + 0x2c).wrapping_add(0x24u32.wrapping_mul(idx as u32));
    m.set_f64(at, px);
    m.set_f64(at + 8, py);
    m.set_f64(at + 0x10, pz);
    m.set_f64(at + 0x18, w);
    m.set_u32(at + 0x20, 0);
    m.set_i32(c + 4, m.i32(c + 4).wrapping_add(1));
    let tw = t.to_bits();
    o.call(m, NURBS_CRV_ADD_KNOT_NON_UNIFORM, &[c, idx as u32, tw as u32, (tw >> 32) as u32]);
    knots_crv_reparametrization(m, c)?;
    if m.i16(c + 0x4c) == 1 {
        let owner = o.call(m, SI_FIND_NURBS_PATH_FCURVE_OWNER, &[c]).eax;
        o.call(m, SI_ADD_FCURVE_PATH_KEY, &[owner, idx.wrapping_sub(1) as u32]);
    }
    Ok(())
}

/// [soft 0x084eed8c knotsWithoutMultiplicity], on the image (`nrb_point.c`):
/// `knotsWithoutMultiplicity(knots, n, &count)` answers a new array (`si_knotsAllocate(n)`) of
/// the knots with repeats removed: the first knot is copied (`fld`, `fstp`, even for n below
/// 1); a later knot is kept when its difference from the last one kept, taken in the register,
/// has an absolute value above 1e-6 (0x086619c8; a NaN fails the test). `*count` is the
/// number kept minus one.
pub fn knots_without_multiplicity(m: &mut Mem, knots: u32, n: i32, count: u32) -> u32 {
    let (out, kept) = knots_distinct(m, knots, n);
    m.set_i32(count, kept);
    out
}

/// The body of [`knots_without_multiplicity`]: the new array and the count it stores.
fn knots_distinct(m: &mut Mem, knots: u32, n: i32) -> (u32, i32) {
    use si_x87::x;
    let out = knots_allocate(m, n);
    let mut kept: i32 = 1;
    mov64(m, knots, out);
    let eps = x(1e-6f64);
    let mut i: i32 = 1;
    while i < n {
        let k = x(m.f64(at8(knots, i)));
        let d = (k - x(m.f64(at8(out, kept.wrapping_sub(1))))).abs();
        if d.gt(eps) {
            m.set_f64(at8(out, kept), k.to_f64());
            kept = kept.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    (out, kept.wrapping_sub(1))
}

/// `si_trmSrfHitInd (0x084e497c)`, `nrb_draw.c`: not ported; goes through `o`.
pub const SI_TRM_SRF_HIT_IND: Routine = Routine::new("si_trmSrfHitInd", 0x084e_497c);

/// [soft 0x084eedfc SI_srfHitDirAndVal], on the image (`nrb_point.c`):
/// `SI_srfHitDirAndVal(srf, index, &dir, &value, second)` says which display line sample the
/// running index `index` of a surface without trims (+0x60 not above 0) is, where the
/// samples are counted along the lines of the display record (+0x80 lines of 0x14 bytes at
/// +0x84: the sample count first, the samples, 0x28 bytes each, at +0x10). The U lines'
/// samples come first (`first` = line 0's sample count × (distinct U knots × the U steps at
/// +0x7c, plus 1 when U is open)); `*dir` = 0 and the V line's number and place are
/// `index / count`, `index % count`; then the V lines (`*dir` = 1, after
/// `SI_nurbsSrfNoVlines` lines have been skipped); past them the projected curves' lines
/// (+0x68 of them, the last ones of the list): `*dir` = 2 + the curve's number, or −1 past all,
/// and nothing else is written. For a sample found, `*value` is the first or (with `second`
/// not 0) the second double of the 0x28-byte sample through the unit (a line not found leaves
/// 0.0). The distinct knots come from [`knots_without_multiplicity`] (U knots +0xc/+8, V
/// +0x30/+0x2c; both blocks freed, V's first). `idiv` by zero traps in the original: refused.
pub fn srf_hit_dir_and_val(m: &mut Mem, s: u32, index: i32, dir_out: u32, val_out: u32, second: u16) -> Result<(), Halt> {
    let mut val: f64 = 0.0;
    let (mut per_u, mut per_v) = (0i32, 0i32);
    if m.i32(s + 0x60) > 0 {
        return Ok(());
    }
    // SI_nurbsSrfNoVlines (own::srf_no_vlines, read here from the image)
    let no_vlines = if m.i16(s + 0x20) == 0 {
        m.i32(s + 8).wrapping_sub(m.i32(s).wrapping_sub(2).wrapping_mul(2).wrapping_add(1)).wrapping_mul(m.i32(s + 0x7c)).wrapping_add(1)
    } else {
        m.i32(s + 8).wrapping_sub(1).wrapping_mul(m.i32(s + 0x7c))
    };
    let lines = m.u32(s + 0x84);
    if lines != 0 {
        per_u = m.i32(lines);
        per_v = m.i32(lines.wrapping_add(20u32.wrapping_mul(no_vlines as u32)));
    }
    let (ku, cu) = knots_distinct(m, m.u32(s + 0xc), m.i32(s + 8));
    let (kv, cv) = knots_distinct(m, m.u32(s + 0x30), m.i32(s + 0x2c));
    free_some(m, kv);
    free_some(m, ku);
    let mut cu = cu.wrapping_mul(m.i32(s + 0x7c));
    let mut cv = cv.wrapping_mul(m.i32(s + 0x78));
    if m.i16(s + 0x20) == 0 {
        cu = cu.wrapping_add(1);
    }
    if m.i16(s + 0x44) == 0 {
        cv = cv.wrapping_add(1);
    }
    let first = per_u.wrapping_mul(cu);
    let second_n = per_v.wrapping_mul(cv);
    let div = |a: i32, b: i32| -> Result<(i32, i32), Halt> {
        if b == 0 || (b == -1 && a == i32::MIN) {
            Err(Halt::Refused("SI_srfHitDirAndVal: division by zero"))
        } else {
            Ok((a.wrapping_div(b), a.wrapping_rem(b)))
        }
    };
    let (mut line, quo, rem);
    if index < first {
        m.set_i32(dir_out, 0);
        (quo, rem) = div(index, per_u)?;
        line = 0;
    } else {
        let upto = second_n.wrapping_add(first);
        if index < upto {
            m.set_i32(dir_out, 1);
            (quo, rem) = div(index.wrapping_sub(first), per_v)?;
            line = no_vlines;
        } else {
            m.set_i32(dir_out, -1);
            let proj = m.i32(s + 0x68);
            let mut total = upto;
            let mut c: i32 = 0;
            if c >= proj {
                return Ok(());
            }
            let mut rec = m.u32(s + 0x84).wrapping_add(m.i32(s + 0x80).wrapping_sub(proj).wrapping_mul(0x14) as u32);
            loop {
                total = total.wrapping_add(m.i32(rec));
                if index < total {
                    m.set_i32(dir_out, c.wrapping_add(2));
                    return Ok(());
                }
                rec = rec.wrapping_add(0x14);
                c = c.wrapping_add(1);
                if c >= proj {
                    return Ok(());
                }
            }
        }
    }
    let count = m.i32(s + 0x80);
    let mut seen: i32 = 0;
    loop {
        if line >= count.wrapping_sub(1) {
            break;
        }
        if m.i32(m.u32(s + 0x84).wrapping_add(20u32.wrapping_mul(line as u32))) != 0 {
            seen = seen.wrapping_add(1);
        }
        if seen == quo.wrapping_add(1) {
            break;
        }
        line = line.wrapping_add(1);
    }
    if line < count {
        let rec = m.u32(s + 0x84).wrapping_add(20u32.wrapping_mul(line as u32));
        if m.i32(rec) != 0 {
            let sample = m.u32(rec + 0x10).wrapping_add(40u32.wrapping_mul(rem as u32));
            val = si_x87::fmov64(m.f64(sample.wrapping_add(if second != 0 { 8 } else { 0 })));
        }
        m.set_f64(val_out, si_x87::fmov64(val));
    } else {
        m.set_f64(val_out, si_x87::fmov64(val));
    }
    Ok(())
}

/// [soft 0x084ef008 SI_nurbsSrfIndHit], on the image (`nrb_point.c`):
/// `SI_nurbsSrfIndHit(srf, index, second, &a, &b, &value)`: for a surface without trims
/// ([`srf_hit_dir_and_val`] with (srf, index, a, value, second)), and when it found a line
/// beyond the first two kinds (`*a` above 1: a projected curve) `*b` = `*a − 2` and `*a` = 3;
/// with trims `si_trmSrfHitInd (0x084e497c)` (not ported) through `o` with (srf, index,
/// second as a signed 16-bit value widened, a, b, value).
pub fn srf_ind_hit(m: &mut Mem, o: &mut dyn Outside, s: u32, index: i32, second: i16, a: u32, b: u32, value: u32) -> Result<(), Halt> {
    if m.i32(s + 0x60) == 0 {
        srf_hit_dir_and_val(m, s, index, a, value, second as u16)?;
        let v = m.i32(a);
        if v > 1 {
            m.set_i32(b, v.wrapping_sub(2));
            m.set_i32(a, 3);
        }
    } else {
        o.call(m, SI_TRM_SRF_HIT_IND, &[s, index as u32, second as i32 as u32, a, b, value]);
    }
    Ok(())
}
