// SPDX-License-Identifier: MIT
//! Patches: the surface counterpart of the spline (finding 0020, sections 10 to 13).
//!
//! A patch (`SI_Patch`, 0x204 bytes: [`si_types::geom::Patch`]) is a grid of `nb_u × nb_v`
//! control points with a spline type, a closed flag, a tension and a step count **in each
//! direction**. A [`Surface`] is that record with the arrays it points to held beside it.
//!
//! - **Points** are 16-byte records (x, y, z and a fourth float that the readers set to 1
//!   and no routine ported here reads); point (i, j) is at index `i × nb_v + j`: U is the
//!   major direction.
//! - **Cells.** Each direction is cut into segments exactly as `SI_splineNbSegmentStep`
//!   counts them ([`crate::eval::nb_segment_step`]): a Bézier direction takes its control
//!   points three to a segment (the old file layout of a spline, not keys with handles),
//!   the other types one. A cell is one segment in U by one in V and is evaluated from the
//!   4 × 4 points starting at `(cell_u × step_u, cell_v × step_v)`, each index wrapping
//!   round its count.
//! - **A point of a cell** at the local parameters (u, v) is `U × Bu × G × Bvᵀ × Vᵀ` per
//!   coordinate, with `U = [u³ u² u 1]`, `V = [v³ v² v 1]`, `B` the basis of the direction's
//!   type ([`crate::basis`]) and `G` the 4 × 4 coordinates.
//! - **The global parameter** of [`value`] runs over `[0, segments]` in each direction; the
//!   whole part picks the cell, but a parameter that is exactly a whole number above 0
//!   belongs to the cell **before** it (u = 1 on that cell), unlike a spline's.
//!
//! Arithmetic follows the original's x87 sequences on [`si_x87::F80`]; each function says
//! where values are rounded to float.

use si_math::algebra::{matrix_copy, matrix_multiply, matrix_transpose};
use si_math::types::Matrix;
use si_types::geom::Patch;
use si_types::Ptr32;
use si_x87::{fmov, x, F80};

use crate::basis::{matrix_precision, spline_basis_matrix};
use crate::eval::{nb_segment_step, wrap_down, wrap_up};
use crate::{held, Vector};

/// A patch with the arrays it owns. A pointer of the record is null exactly when its array
/// here is not allocated (an empty `Vec` with a non-null pointer is a block of no elements,
/// which the original cannot have: its `SI_calloc` returns null for a count of 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub rec: Patch,
    /// `rec.points`: `nb_u × nb_v` records of four floats, index `i × nb_v + j`.
    pub points: Vec<[f32; 4]>,
    /// `rec.tags`: one word a point; bit 0: the point is tagged (selected).
    pub tags: Vec<i16>,
    /// `rec.surface0`: the display curves that run in U, `surface0_dims[1]` curves of
    /// `surface0_dims[0]` points each ([`crate::patch_grid::compute_surface`]).
    pub surface0: Vec<Vector>,
    /// `rec.modif0`: one word a point of `surface0`, 1 where the last partial update wrote.
    pub modif0: Vec<i16>,
    /// `rec.surface1`: the display curves that run in V.
    pub surface1: Vec<Vector>,
    pub modif1: Vec<i16>,
    /// `rec.subdiv0`: the points of the shading grid, `(u_step + 1) × (v_step + 1)` a cell
    /// ([`crate::patch_grid::computes_subdivision`]).
    pub subdiv0: Vec<Vector>,
    /// `rec.subdiv1`: the unit normals at those points.
    pub subdiv1: Vec<Vector>,
}

impl Default for Surface {
    fn default() -> Self {
        Surface::new()
    }
}

pub(crate) const V0: Vector = Vector { x: 0.0, y: 0.0, z: 0.0 };

impl Surface {
    /// A new patch without points: what `SI_patchAllocate` leaves ([`Patch::allocate`]).
    pub fn new() -> Surface {
        Surface {
            rec: Patch::allocate(),
            points: Vec::new(),
            tags: Vec::new(),
            surface0: Vec::new(),
            modif0: Vec::new(),
            surface1: Vec::new(),
            modif1: Vec::new(),
            subdiv0: Vec::new(),
            subdiv1: Vec::new(),
        }
    }

    /// A patch from a grid of points (`points[i × nb_v + j]`), with the matrices computed.
    pub fn from_points(kinds: [i32; 2], closed: [bool; 2], nb_u: u16, nb_v: u16, points: &[[f32; 3]]) -> Surface {
        let mut s = Surface::new();
        s.rec.u_type = kinds[0];
        s.rec.v_type = kinds[1];
        s.rec.u_closed = closed[0] as i16;
        s.rec.v_closed = closed[1] as i16;
        s.rec.nb_u = nb_u;
        s.rec.nb_v = nb_v;
        s.set_points(points);
        update_matrix(&mut s);
        s
    }

    fn set_points(&mut self, points: &[[f32; 3]]) {
        let n = self.rec.nb_u as usize * self.rec.nb_v as usize;
        self.points = (0..n).map(|i| points.get(i).map_or([0.0, 0.0, 0.0, 1.0], |p| [p[0], p[1], p[2], 1.0])).collect();
        self.tags = vec![0; n];
        self.rec.points = if n == 0 { Ptr32::NULL } else { held() };
        self.rec.tags = if n == 0 { Ptr32::NULL } else { held() };
    }

    /// The patch a model file holds (`si-hrc`, finding 0004) as the reader
    /// (`patchBinaryRead`, 0x083f0690) leaves it in memory: an allocated record with the
    /// file's fields over it, the points in 16-byte records with a fourth float of 1, the
    /// tags, and the matrices from [`update_matrix`] (the reader's last step). The display
    /// curves are not built by the reader; the model code calls
    /// [`crate::patch_grid::update_surface`] afterwards.
    ///
    /// A file cut short inside the points gives fewer points than `nb_u × nb_v`; the
    /// original's block is then zero beyond them (it comes from `SI_calloc`), and so is
    /// this one, fourth float included.
    pub fn from_file(p: &si_hrc::Patch) -> Surface {
        let mut s = Surface::new();
        s.rec.u_type = p.kind[0];
        s.rec.v_type = p.kind[1];
        s.rec.nb_u = p.nu;
        s.rec.nb_v = p.nv;
        s.rec.u_closed = p.close[0];
        s.rec.v_closed = p.close[1];
        s.rec.u_tension = p.tension[0];
        s.rec.v_tension = p.tension[1];
        s.rec.u_step = p.step[0] as u16;
        s.rec.v_step = p.step[1] as u16;
        s.rec.u_nb_curves = p.curves[0] as u16;
        s.rec.v_nb_curves = p.curves[1] as u16;
        s.rec.u_bottom = p.caps[0];
        s.rec.u_top = p.caps[1];
        s.rec.v_bottom = p.caps[2];
        s.rec.v_top = p.caps[3];
        s.rec.unk_1d4 = p.faceted;
        s.rec.mr_approx = p.approx;
        s.rec.mr_view = p.view;
        s.rec.mr_edge = p.edge;
        s.rec.mr_ucurv = p.curvature[0];
        s.rec.mr_vcurv = p.curvature[1];
        s.rec.mr_min = p.rec_min;
        s.rec.mr_max = p.rec_max;
        s.rec.mr_rec = p.recursion;
        let n = p.nu as usize * p.nv as usize;
        s.points = vec![[0.0; 4]; n];
        for (d, f) in s.points.iter_mut().zip(&p.points) {
            *d = [f[0], f[1], f[2], 1.0];
        }
        s.tags = vec![0; n];
        for (d, f) in s.tags.iter_mut().zip(&p.tags) {
            *d = *f;
        }
        s.rec.points = if n == 0 { Ptr32::NULL } else { held() };
        s.rec.tags = if n == 0 { Ptr32::NULL } else { held() };
        update_matrix(&mut s);
        s
    }

    /// Point `index` of the block, or `None` where the original would read outside it.
    pub(crate) fn point(&self, index: i32) -> Option<[f32; 4]> {
        if index < 0 {
            return None;
        }
        self.points.get(index as usize).copied()
    }

    /// Segments and control points a segment in U and in V:
    /// `SI_splineNbSegmentStep` of each direction.
    pub fn cells(&self) -> ((i32, i32), (i32, i32)) {
        cells_of(&self.rec)
    }
}

/// Segments and control points a segment in U and in V of a patch record.
pub(crate) fn cells_of(r: &si_types::geom::Patch) -> ((i32, i32), (i32, i32)) {
    (nb_segment_step(r.u_type, r.nb_u as i32, r.u_closed), nb_segment_step(r.v_type, r.nb_v as i32, r.v_closed))
}

/// Where a patch's record and control points are read from: a [`Surface`] (the value
/// forms) or the memory image (`patch_eval_image`, where the points are read at their
/// addresses, also where the original reads outside its block).
pub trait PatchSource {
    /// The record.
    fn rec(&self) -> &si_types::geom::Patch;
    /// Control point `index` (x, y, z and the fourth float); `None` where the original
    /// would fault.
    fn point(&self, index: i32) -> Option<[f32; 4]>;
    /// Segments and control points a segment in U and in V.
    fn cells(&self) -> ((i32, i32), (i32, i32)) {
        cells_of(self.rec())
    }
}

impl PatchSource for Surface {
    fn rec(&self) -> &si_types::geom::Patch {
        &self.rec
    }
    fn point(&self, index: i32) -> Option<[f32; 4]> {
        Surface::point(self, index)
    }
}

/// `float + float` rounded to float, as a `fld` / `fadd` / `fstp` triple does it.
pub(crate) fn addf(a: f32, b: f32) -> f32 {
    (x(a) + b).to_f32()
}

/// The six matrices of a patch [soft 0x0847a680 SI_patchUpdateMatrix].
///
/// A linear direction first gets its step count forced to 1. Then, with `Bu`, `Bv` the
/// bases of the two types and `E(n)` the forward-difference matrix of n steps
/// ([`matrix_precision`]):
///
/// | Field | Value |
/// |---|---|
/// | `basis[0]` (+0x24) | `Bu` |
/// | `basis[1]` (+0x64) | `Bvᵀ` |
/// | `matrices[0]` (+0xa4) | `E(v_nb_curves) × Bu` |
/// | `matrices[1]` (+0xe4) | `E(u_nb_curves) × Bv` |
/// | `matrices[2]` (+0x124) | `(E(v_step) × Bv)ᵀ` |
/// | `matrices[3]` (+0x164) | `(E(u_step) × Bu)ᵀ` |
///
/// Both bases are written into the same local matrix, and a type outside 0..=3 writes
/// nothing: an unknown V type therefore uses the U basis, and an unknown U type whatever
/// the stack held (zeros here).
pub fn update_matrix(s: &mut Surface) {
    update_matrix_rec(&mut s.rec);
}

/// [`update_matrix`] on the record alone (it reads and writes nothing else).
pub(crate) fn update_matrix_rec(r: &mut si_types::geom::Patch) {
    if r.u_type == 0 {
        r.u_step = 1;
    }
    if r.v_type == 0 {
        r.v_step = 1;
    }
    let mut b = [[0.0f32; 4]; 4];
    spline_basis_matrix(r.u_type, r.u_tension, &mut b);
    r.basis[0] = matrix_copy(&b);
    r.matrices[0] = matrix_multiply(&matrix_precision(r.v_nb_curves as i32), &b);
    r.matrices[3] = matrix_transpose(&matrix_multiply(&matrix_precision(r.u_step as i32), &b));
    spline_basis_matrix(r.v_type, r.v_tension, &mut b);
    r.basis[1] = matrix_transpose(&b);
    r.matrices[1] = matrix_multiply(&matrix_precision(r.u_nb_curves as i32), &b);
    r.matrices[2] = matrix_transpose(&matrix_multiply(&matrix_precision(r.v_step as i32), &b));
}

/// The number of triangles of the shading grid [soft 0x0847de7c SI_patchNbTriangles]:
/// `segments_u × segments_v × u_step × v_step × 2`, in 32-bit arithmetic.
pub fn nb_triangles(s: &Surface) -> i32 {
    let ((nsu, _), (nsv, _)) = s.cells();
    nsu.wrapping_mul(nsv).wrapping_mul(s.rec.u_step as i32).wrapping_mul(s.rec.v_step as i32).wrapping_mul(2)
}

/// How many turns of a peeling loop are followed literally before its end is computed.
const PEEL_BUDGET: u32 = 1 << 12;

/// `while t > 1 { t = float(t − 1); index += step }`: the loop that takes the whole part
/// off a patch parameter and counts control points.
///
/// The comparison is strict, so a parameter of exactly k (k ≥ 1) ends as 1 on cell k − 1.
/// Up to 2²⁴ every subtraction is exact and the port computes the end directly once the
/// budget is spent. **Above 2²⁴ the subtraction soon stops changing the float and the
/// original does not return**; `None` then.
pub(crate) fn peel(mut t: F80, step: i32) -> Option<(F80, i32)> {
    let mut index = 0i32;
    let mut turns = 0u32;
    while t.gt(F80::ONE) {
        if turns >= PEEL_BUDGET && !t.gt(x(16_777_216.0f32)) {
            // t is a float here (it has been through the loop) and at most 2²⁴: the
            // remaining turns are exact
            let f = t.to_f32();
            let n = f.ceil() as i64 - 1;
            let rest = (x(f) - F80::from_i64(n)).to_f32();
            return Some((x(rest), index.wrapping_add((n as i32).wrapping_mul(step))));
        }
        let n = x((t - F80::ONE).to_f32());
        if n.to_bits() == t.to_bits() {
            return None;
        }
        t = n;
        index = index.wrapping_add(step);
        turns += 1;
    }
    Some((t, index))
}

/// The 4 × 4 control points of the cell that starts at control point `(iu, iv)`, one matrix
/// a coordinate, `[i][j]` being point `((iu + i) mod nb_u, (iv + j) mod nb_v)`.
///
/// `short_index`: the two evaluators narrow the point's index to 16 bits (signed) before
/// they use it, the grid builders do not. `None` where the original divides by zero (a
/// direction without points) or reads outside its block (an index of 32,768 or more
/// narrowed to a negative number).
pub(crate) fn gather(s: &(impl PatchSource + ?Sized), iu: i32, iv: i32, short_index: bool) -> Option<[Matrix; 3]> {
    let (nu, nv) = (s.rec().nb_u as i32, s.rec().nb_v as i32);
    if nu == 0 || nv == 0 {
        return None;
    }
    let mut g = [[[0.0f32; 4]; 4]; 3];
    for i in 0..4 {
        for j in 0..4 {
            let row = iu.wrapping_add(i as i32) % nu;
            let col = iv.wrapping_add(j as i32) % nv;
            let mut index = row.wrapping_mul(nv).wrapping_add(col);
            if short_index {
                index = index as i16 as i32;
            }
            let p = s.point(index)?;
            for k in 0..3 {
                g[k][i][j] = fmov(p[k]);
            }
        }
    }
    Some(g)
}

/// The point at the global parameters (u, v) [soft 0x0847dee4 SI_patchValue].
///
/// Nothing is clamped or wrapped: the whole part of each parameter is taken off by
/// [`peel`] and counts cells, the control point indices wrap round the grid, and a
/// parameter below 0 evaluates the first cell's polynomial outside its range. With `M`
/// the U basis (+0x24), `N` the transposed V basis (+0x64), u² and u³ = u² × u in the
/// register and likewise for v:
///
/// - U weights: `w0 = ((u²M₁₀ + u³M₀₀) + uM₂₀) + M₃₀`, `w2 = ((u³M₀₂ + u²M₁₂) + uM₂₂) + M₃₂`,
///   `w3 = (uM₂₃ + (u²M₁₃ + u³M₀₃)) + M₃₃`, all three kept in the register;
///   `w1 = float((uM₂₁ + float(u²M₁₁ + float(u³M₀₁))) + M₃₁)`;
/// - V weights, each `x_r = float((vN_r2 + float(v²N_r1 + float(N_r0 v³))) + N_r3)`;
/// - per coordinate `c_j = w3 G₃ⱼ + ((w0 G₀ⱼ + w1 G₁ⱼ) + w2 G₂ⱼ)` in the register and the
///   result `float(((c₂x₂ + (c₀x₀ + c₁x₁)) + c₃x₃))`.
///
/// `None`: the original divides by zero (no points in a direction), reads outside its
/// block, or does not return ([`peel`]).
pub fn value(s: &(impl PatchSource + ?Sized), u: f32, v: f32) -> Option<Vector> {
    let ((_, stu), (_, stv)) = s.cells();
    let (u, iu) = peel(x(u), stu)?;
    let u2 = u * u;
    let (v, iv) = peel(x(v), stv)?;
    let v = x(v.to_f32());
    let v2 = v * v;
    let g = gather(s, iu, iv, true)?;
    let m = &s.rec().basis[0];
    let n = &s.rec().basis[1];
    let u3 = u2 * u;
    let w0 = (((u2 * m[1][0]) + (u3 * m[0][0])) + (u * m[2][0])) + m[3][0];
    let a = (u3 * m[0][1]).to_f32();
    let a = ((u2 * m[1][1]) + a).to_f32();
    let w1 = (((u * m[2][1]) + a) + m[3][1]).to_f32();
    let w2 = (((u3 * m[0][2]) + (u2 * m[1][2])) + (u * m[2][2])) + m[3][2];
    let w3 = ((u * m[2][3]) + ((u2 * m[1][3]) + (u3 * m[0][3]))) + m[3][3];
    let v3 = v2 * v;
    let xw = |r: usize| {
        let a = (x(n[r][0]) * v3).to_f32();
        let a = ((x(n[r][1]) * v2) + a).to_f32();
        (((v * n[r][2]) + a) + n[r][3]).to_f32()
    };
    let xs = [xw(0), xw(1), xw(2), xw(3)];
    let at = |g: &Matrix| {
        let c = |j: usize| (w3 * g[3][j]) + (((w0 * g[0][j]) + (x(w1) * g[1][j])) + (w2 * g[2][j]));
        (((c(2) * xs[2]) + ((c(0) * xs[0]) + (c(1) * xs[1]))) + (c(3) * xs[3])).to_f32()
    };
    Some(Vector { x: at(&g[0]), y: at(&g[1]), z: at(&g[2]) })
}

/// A point with the two parametric derivatives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tangents {
    pub point: Vector,
    /// The derivative with respect to u (the original's fifth argument).
    pub tu: Vector,
    /// The derivative with respect to v (its sixth).
    pub tv: Vector,
}

/// Point and derivatives at (u, v) [soft 0x0847ff50 SI_patchValueAndTangent].
///
/// `None` is the original's result 0 with its three destinations untouched: a patch
/// without a segment in one direction. Otherwise, per direction, with n its segments:
///
/// - closed: while the parameter is above n, n is subtracted; then while it is below 0, n is
///   added; **in the register, never rounded to float**;
/// - open: a parameter above n becomes n and one below 0 becomes 0, and the excess
///   (`float(u − n)`, or u itself) is kept: the point is then **extrapolated along the
///   tangent**, `point += excess × derivative`, after the evaluation at the end;
/// - then the whole part is peeled off as in [`value`].
///
/// The evaluation is [`value`]'s with other rounding points (the comment in the body lists
/// them: u² and v² are rounded to float first, three of the four U weights are floats, the
/// second column sum of x and of y is a float and that of z is not), and the derivative
/// weights `float(3u²)`, `float(2u)`, 1 and **0 times the fourth basis row** (the product
/// is formed, so an infinite or NaN element of the basis spoils the derivative).
///
/// Also `None` where the original does not return ([`peel`]), divides by zero or reads
/// outside its block ([`gather`]).
pub fn value_and_tangent(s: &(impl PatchSource + ?Sized), u: f32, v: f32) -> Option<Tangents> {
    let ((nsu, stu), (nsv, stv)) = s.cells();
    if nsu <= 0 || nsv <= 0 {
        return None;
    }
    let keep: fn(F80, F80) -> bool = |t, n| n.lt(t);
    let end: fn(F80, F80) -> F80 = |r, n| if r.eq(F80::ZERO) { n } else { r };
    let map = |t: f32, n: i32, closed: i16| -> (F80, Option<f32>) {
        let n = F80::from_i32(n);
        let mut t = x(t);
        let mut excess = None;
        if n.lt(t) {
            if closed != 0 {
                t = wrap_down(t, n, keep, end);
            } else {
                excess = Some((t - n).to_f32());
                t = n;
            }
        }
        if F80::ZERO.gt(t) {
            if closed != 0 {
                t = wrap_up(t, n);
            } else {
                excess = Some(t.to_f32());
                t = F80::ZERO;
            }
        }
        (t, excess)
    };
    let (u, eu) = map(u, nsu, s.rec().u_closed);
    let (v, ev) = map(v, nsv, s.rec().v_closed);
    let (u, iu) = peel(u, stu)?;
    let uu = (u * u).to_f32();
    let (v, iv) = peel(v, stv)?;
    let vv = (v * v).to_f32();
    let g = gather(s, iu, iv, true)?;
    let m = &s.rec().basis[0];
    let n = &s.rec().basis[1];

    // U: weights of the point (w0 in the register, the others floats) and of the derivative
    let u2f = (u + u).to_f32();
    let uuu = x(uu) * u;
    let u3f = (x(3.0f64) * uu).to_f32();
    let w0 = (((x(uu) * m[1][0]) + (x(m[0][0]) * uuu)) + (u * m[2][0])) + m[3][0];
    let a = ((x(m[0][1]) * uuu) + (x(uu) * m[1][1])).to_f32();
    let a = ((u * m[2][1]) + a).to_f32();
    let w1 = (x(a) + m[3][1]).to_f32();
    let a = ((x(m[0][2]) * uuu) + (x(uu) * m[1][2])).to_f32();
    let w2 = (((u * m[2][2]) + a) + m[3][2]).to_f32();
    let a = (uuu * m[0][3]).to_f32();
    let w3 = ((((x(uu) * m[1][3]) + a) + (u * m[2][3])) + m[3][3]).to_f32();
    let a = (x(u3f) * m[0][1]).to_f32();
    let a = (((x(u2f) * m[1][1]) + a) + m[2][1]).to_f32();
    let z = (F80::ZERO * m[3][1]).to_f32();
    let dw1 = (x(a) + z).to_f32();
    let a = (x(u3f) * m[0][2]).to_f32();
    let a = (((x(u2f) * m[1][2]) + a) + m[2][2]).to_f32();
    let dw2 = ((x(m[3][2]) * F80::ZERO) + a).to_f32();
    let dw0 = (F80::ZERO * m[3][0]) + (((x(u3f) * m[0][0]) + (x(u2f) * m[1][0])) + m[2][0]);
    let dw3 = ((F80::ZERO * m[3][3]) + (((x(u3f) * m[0][3]) + (x(u2f) * m[1][3])) + m[2][3])).to_f32();

    // V: all floats
    let v2f = (v + v).to_f32();
    let v3f = (x(3.0f64) * vv).to_f32();
    let vvv = x(vv) * v;
    let a = (x(n[0][0]) * vvv).to_f32();
    let a = ((x(n[0][1]) * vv) + a).to_f32();
    let x0 = (((x(n[0][2]) * v) + a) + n[0][3]).to_f32();
    let mid = |r: usize| {
        let a = ((x(n[r][0]) * vvv) + (x(n[r][1]) * vv)).to_f32();
        (((x(n[r][2]) * v) + a) + n[r][3]).to_f32()
    };
    let (x1, x2) = (mid(1), mid(2));
    let a = (vvv * n[3][0]).to_f32();
    let a = ((x(n[3][1]) * vv) + a).to_f32();
    let a = ((v * n[3][2]) + a).to_f32();
    let x3 = (x(a) + n[3][3]).to_f32();
    let a = (x(n[0][0]) * v3f).to_f32();
    let b = (x(n[0][1]) * v2f).to_f32();
    let a = ((x(a) + b) + n[0][2]).to_f32();
    let z = (F80::ZERO * n[0][3]).to_f32();
    let dx0 = (x(a) + z).to_f32();
    let a = (x(n[1][0]) * v3f).to_f32();
    let b = (x(n[1][1]) * v2f).to_f32();
    let a = ((x(a) + b) + n[1][2]).to_f32();
    let dx1 = ((x(n[1][3]) * F80::ZERO) + a).to_f32();
    let dx2 = ((F80::ZERO * n[2][3]) + (((x(n[2][0]) * v3f) + (x(n[2][1]) * v2f)) + n[2][2])).to_f32();
    let a = (((x(n[3][0]) * v3f) + (x(n[3][1]) * v2f)) + n[3][2]).to_f32();
    let dx3 = ((x(n[3][3]) * F80::ZERO) + a).to_f32();

    // column sums of the point, in the register
    let col = |g: &Matrix, j: usize| (x(w3) * g[3][j]) + ((x(w2) * g[2][j]) + ((w0 * g[0][j]) + (x(w1) * g[1][j])));
    // x: the second column goes through a float three times
    let gx = &g[0];
    let c1 = ((w0 * gx[0][1]) + (x(w1) * gx[1][1])).to_f32();
    let c1 = ((x(w2) * gx[2][1]) + c1).to_f32();
    let c1 = ((x(w3) * gx[3][1]) + c1).to_f32();
    let (c0, c2, c3) = (col(gx, 0), col(gx, 2), col(gx, 3));
    let mut px = ((((c0 * x0) + (x(c1) * x1)) + (c2 * x2)) + (c3 * x3)).to_f32();
    let tvx = ((c3 * dx3) + (((x(c1) * dx1) + (c0 * dx0)) + (c2 * dx2))).to_f32();
    // y: four times
    let gy = &g[1];
    let c1 = (w0 * gy[0][1]).to_f32();
    let c1 = ((x(w1) * gy[1][1]) + c1).to_f32();
    let c1 = ((x(w2) * gy[2][1]) + c1).to_f32();
    let c1 = ((x(w3) * gy[3][1]) + c1).to_f32();
    let c0 = (((x(w1) * gy[1][0]) + (w0 * gy[0][0])) + (x(w2) * gy[2][0])) + (x(w3) * gy[3][0]);
    let (c2, c3) = (col(gy, 2), col(gy, 3));
    let mut py = ((((c0 * x0) + (x(c1) * x1)) + (c2 * x2)) + (c3 * x3)).to_f32();
    let tvy = ((c3 * dx3) + (((x(c1) * dx1) + (c0 * dx0)) + (c2 * dx2))).to_f32();
    // z: not at all
    let gz = &g[2];
    let c2 = (x(w3) * gz[3][2]) + (((w0 * gz[0][2]) + (x(w1) * gz[1][2])) + (x(w2) * gz[2][2]));
    let (c0, c1, c3) = (col(gz, 0), col(gz, 1), col(gz, 3));
    let mut pz = (((c2 * x2) + ((c1 * x1) + (c0 * x0))) + (c3 * x3)).to_f32();
    let tvz = ((c3 * dx3) + ((c2 * dx2) + ((c1 * dx1) + (c0 * dx0)))).to_f32();

    // the derivative in u
    let dcol = |g: &Matrix, j: usize| (x(dw3) * g[3][j]) + ((x(dw2) * g[2][j]) + ((dw0 * g[0][j]) + (x(dw1) * g[1][j])));
    let du = |g: &Matrix| ((dcol(g, 2) * x2) + ((dcol(g, 1) * x1) + (dcol(g, 0) * x0))) + (dcol(g, 3) * x3);
    let tux80 = du(gx);
    let tu = Vector { x: tux80.to_f32(), y: du(gy).to_f32(), z: du(gz).to_f32() };
    let tv = Vector { x: tvx, y: tvy, z: tvz };

    if let Some(e) = eu {
        // the x term uses the derivative as it still is in the register
        px = ((tux80 * e) + px).to_f32();
        py = ((x(e) * tu.y) + py).to_f32();
        pz = ((x(e) * tu.z) + pz).to_f32();
    }
    if let Some(e) = ev {
        px = ((x(e) * tv.x) + px).to_f32();
        py = ((x(e) * tv.y) + py).to_f32();
        pz = ((x(e) * tv.z) + pz).to_f32();
    }
    Some(Tangents { point: Vector { x: px, y: py, z: pz }, tu, tv })
}

/// Point and unit normal at (u, v) [soft 0x0847fe84 SI_patchValueAndNormal].
///
/// The normal is `tv × tu` of [`value_and_tangent`], the three products' differences kept
/// in the register; its length is `float(√((y² + x²) + z²))`, and when the absolute length
/// is above the double 0.0001 each component is divided by it. Otherwise the cross product
/// is returned as it is. The original always returns 1 and leaves both destinations alone
/// when the tangent routine refuses: `None`.
pub fn value_and_normal(s: &(impl PatchSource + ?Sized), u: f32, v: f32) -> Option<(Vector, Vector)> {
    let t = value_and_tangent(s, u, v)?;
    let (a, b) = (t.tu, t.tv);
    let nx = (x(b.y) * a.z) - (x(b.z) * a.y);
    let nz = (x(a.y) * b.x) - (x(a.x) * b.y);
    let ny = (x(b.z) * a.x) - (x(a.z) * b.x);
    let len = (((ny * ny) + (nx * nx)) + (nz * nz)).sqrt().to_f32();
    let n = if x(len).abs().gt(x(1.0e-4f64)) {
        Vector { x: (nx / len).to_f32(), y: (ny / len).to_f32(), z: (nz / len).to_f32() }
    } else {
        Vector { x: nx.to_f32(), y: ny.to_f32(), z: nz.to_f32() }
    };
    Some((t.point, n))
}

/// Applies a matrix to the control points and to both sets of display curves
/// [soft 0x084866a4 SI_patchTransform]: each point becomes
/// `float((((y m₁ⱼ + x m₀ⱼ) + z m₂ⱼ) + m₃ⱼ))`; the fourth float of a control point is left
/// alone. The shading grid is marked stale (+0x1c0 = 0), not transformed.
pub fn transform(s: &mut Surface, m: &Matrix) {
    let apply = |p: [f32; 3]| -> [f32; 3] {
        core::array::from_fn(|j| ((((x(p[1]) * m[1][j]) + (x(p[0]) * m[0][j])) + (x(p[2]) * m[2][j])) + m[3][j]).to_f32())
    };
    let n = s.rec.nb_u as usize * s.rec.nb_v as usize;
    for p in s.points.iter_mut().take(n) {
        let q = apply([p[0], p[1], p[2]]);
        (p[0], p[1], p[2]) = (q[0], q[1], q[2]);
    }
    let dims = [s.rec.surface0_dims, s.rec.surface1_dims];
    let nulls = [s.rec.surface0.is_null(), s.rec.surface1.is_null()];
    for (k, curves) in [&mut s.surface0, &mut s.surface1].into_iter().enumerate() {
        if nulls[k] {
            continue;
        }
        let n = dims[k][0] as usize * dims[k][1] as usize;
        for p in curves.iter_mut().take(n) {
            let q = apply([p.x, p.y, p.z]);
            *p = Vector { x: q[0], y: q[1], z: q[2] };
        }
    }
    s.rec.unk_1c0 = 0;
}

/// Copies the control points (all four floats) and both sets of display curves of `from`
/// into `to` [soft 0x084810cc SI_patchTransferPoints]. The counts are all computed from
/// `from`; `to` must have the same shape (the original does not check, and neither array
/// of `to` is reallocated). The curve counts are recomputed from the types and steps, not
/// taken from the stored dimensions. Where `to` is shorter, the port stops at its end.
pub fn transfer_points(from: &Surface, to: &mut Surface) {
    let n = from.rec.nb_u as usize * from.rec.nb_v as usize;
    for (d, p) in to.points.iter_mut().zip(from.points.iter().take(n)) {
        *d = *p;
    }
    let ((nsu, _), (nsv, _)) = from.cells();
    let rows = (from.rec.u_nb_curves as i32).wrapping_mul(nsv) + (from.rec.v_closed == 0) as i32;
    let n0 = ((from.rec.u_step as i32).wrapping_mul(nsu).wrapping_add(1)).wrapping_mul(rows);
    for (d, p) in to.surface0.iter_mut().zip(from.surface0.iter().take(n0.max(0) as usize)) {
        *d = *p;
    }
    let rows = (from.rec.v_nb_curves as i32).wrapping_mul(nsu) + (from.rec.u_closed == 0) as i32;
    let n1 = ((from.rec.v_step as i32).wrapping_mul(nsv).wrapping_add(1)).wrapping_mul(rows);
    for (d, p) in to.surface1.iter_mut().zip(from.surface1.iter().take(n1.max(0) as usize)) {
        *d = *p;
    }
}

/// A copy of a patch [soft 0x0847a2b8 SI_patchCopy]: a newly allocated record, the fields
/// listed in the body, the six matrices, the points and tags, and then
/// [`crate::patch_grid::update_surface`] on the copy, which builds its display curves
/// anew. Not copied: the words at +0x000 and +0x1e0, the shading grid and its flag.
pub fn copy(s: &Surface) -> Surface {
    let mut n = Surface::new();
    let (a, b) = (&s.rec, &mut n.rec);
    b.unk_1e4 = a.unk_1e4;
    b.u_type = a.u_type;
    b.v_type = a.v_type;
    b.nb_u = a.nb_u;
    b.nb_v = a.nb_v;
    b.u_closed = a.u_closed;
    b.v_closed = a.v_closed;
    b.u_tension = fmov(a.u_tension);
    b.v_tension = fmov(a.v_tension);
    b.u_step = a.u_step;
    b.v_step = a.v_step;
    b.u_nb_curves = a.u_nb_curves;
    b.v_nb_curves = a.v_nb_curves;
    b.u_top = a.u_top;
    b.u_bottom = a.u_bottom;
    b.v_top = a.v_top;
    b.v_bottom = a.v_bottom;
    b.unk_1d4 = a.unk_1d4;
    b.unk_1d6 = a.unk_1d6;
    b.unk_1ac = a.unk_1ac;
    b.mr_approx = a.mr_approx;
    b.mr_view = a.mr_view;
    b.mr_edge = fmov(a.mr_edge);
    b.mr_ucurv = fmov(a.mr_ucurv);
    b.mr_vcurv = fmov(a.mr_vcurv);
    b.mr_min = a.mr_min;
    b.mr_max = a.mr_max;
    b.mr_rec = a.mr_rec;
    b.basis = [matrix_copy(&a.basis[0]), matrix_copy(&a.basis[1])];
    b.matrices = core::array::from_fn(|i| matrix_copy(&a.matrices[i]));
    let count = a.nb_u as usize * a.nb_v as usize;
    n.points = (0..count).map(|i| s.points.get(i).copied().unwrap_or([0.0; 4])).collect();
    n.tags = (0..count).map(|i| s.tags.get(i).copied().unwrap_or(0)).collect();
    n.rec.points = if count == 0 { Ptr32::NULL } else { held() };
    n.rec.tags = if count == 0 { Ptr32::NULL } else { held() };
    crate::patch_grid::update_surface(&mut n);
    n
}

/// Frees a patch [soft 0x0847a530 SI_patchDispose]: points, tags, `surface0`, `modif0`,
/// `surface1`, `modif1`, the two arrays of the shading grid and the record, in that order,
/// each only when its pointer is set; the caller's pointer is cleared.
pub fn dispose(s: &mut Option<Surface>) {
    *s = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BEZIER, BSPLINE, CARDINAL, LINEAR};

    fn grid(nu: usize, nv: usize) -> Vec<[f32; 3]> {
        let mut p = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                p.push([i as f32, j as f32 * 2.0, ((i * j) as f32) * 0.25 - (i as f32) * 0.5]);
            }
        }
        p
    }

    #[test]
    fn linear_patch_interpolates_its_points() {
        let pts = grid(3, 4);
        let s = Surface::from_points([LINEAR, LINEAR], [false, false], 3, 4, &pts);
        assert_eq!(s.cells(), ((2, 1), (3, 1)));
        for i in 0..3 {
            for j in 0..4 {
                let p = value(&s, i as f32, j as f32).unwrap();
                assert_eq!([p.x, p.y, p.z], pts[i * 4 + j], "{i} {j}");
            }
        }
        let p = value(&s, 0.5, 0.5).unwrap();
        assert_eq!([p.x, p.y], [0.5, 1.0]);
        assert_eq!(nb_triangles(&s), 2 * 3 * 2);
    }

    #[test]
    fn bezier_corners_and_tangent_agree_with_value() {
        let pts = grid(4, 7);
        let s = Surface::from_points([BEZIER, BEZIER], [false, false], 4, 7, &pts);
        assert_eq!(s.cells(), ((1, 3), (2, 3)));
        let p = value(&s, 0.0, 0.0).unwrap();
        assert_eq!([p.x, p.y, p.z], pts[0]);
        let p = value(&s, 1.0, 2.0).unwrap();
        assert_eq!([p.x, p.y, p.z], pts[3 * 7 + 6]);
        let t = value_and_tangent(&s, 0.25, 1.5).unwrap();
        let q = value(&s, 0.25, 1.5).unwrap();
        assert!((t.point.x - q.x).abs() < 1e-5 && (t.point.z - q.z).abs() < 1e-5);
        // beyond the end of an open direction: linear extrapolation along the tangent
        let e = value_and_tangent(&s, 1.5, 1.0).unwrap();
        let b = value_and_tangent(&s, 1.0, 1.0).unwrap();
        assert!((e.point.x - (b.point.x + 0.5 * b.tu.x)).abs() < 1e-5);
        let (_, n) = value_and_normal(&s, 0.5, 0.5).unwrap();
        assert!(((n.x * n.x + n.y * n.y + n.z * n.z) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn closed_directions_wrap_and_whole_parameters_belong_to_the_cell_before() {
        let pts = grid(5, 4);
        let mut s = Surface::from_points([BSPLINE, CARDINAL], [true, false], 5, 4, &pts);
        s.rec.v_tension = 0.5;
        update_matrix(&mut s);
        assert_eq!(s.cells(), ((5, 1), (1, 1)));
        let a = value_and_tangent(&s, 0.5, 0.5).unwrap();
        let b = value_and_tangent(&s, 5.5, 0.5).unwrap();
        let c = value_and_tangent(&s, -4.5, 0.5).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(peel(x(3.0f32), 1).map(|r| (r.0.to_f32(), r.1)), Some((1.0, 2)));
        assert_eq!(peel(x(1.0f32), 1).map(|r| (r.0.to_f32(), r.1)), Some((1.0, 0)));
        assert_eq!(peel(x(1.0e6f32), 3).map(|r| (r.0.to_f32(), r.1)), Some((1.0, 999_999 * 3)));
        assert!(peel(x(f32::INFINITY), 1).is_none());
        assert!(value(&Surface::new(), 0.0, 0.0).is_none());
        assert!(value_and_tangent(&Surface::new(), 0.0, 0.0).is_none());
    }
}
