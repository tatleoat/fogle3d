// SPDX-License-Identifier: MIT
//! Turning a patch into points: the display curves, the grid of points the mesh
//! conversion uses, and the shading grid with normals the renderer uses (finding 0020,
//! section 11).
//!
//! All of it steps by **forward differences in both directions**. For a cell with the
//! 4 × 4 coordinates `G`, `D = E(nu) × Bu × G × (E(nv) × Bv)ᵀ` (each product
//! `SI_matrixMultiply`, every element rounded to float) holds the point at the cell's
//! corner in `D[3][3]`, its forward differences in v along row 3 (third, second, first
//! difference in columns 0, 1, 2) and in u down column 3. One step in v adds, for columns 3,
//! 2, 1 in that order, the column before; one step in u adds, for rows 3, 2, 1, the row
//! before. Every sum is rounded to float.
//!
//! Three consumers, three layouts:
//!
//! | Routine | Used by | Points | Order |
//! |---|---|---|---|
//! | [`patch_surface`] | [`convert_to_mesh`] (the shaded display, the model tools), the renderer for a second patch | `(u_step × segments_u [+1 open]) × (v_step × segments_v [+1 open])`: seams shared | U major |
//! | [`computes_subdivision`] | the renderer (`si_patch2triangle`) | `(u_step + 1) × (v_step + 1)` **a cell**, seams doubled; with unit normals | cell (U major), then U, then V |
//! | [`compute_surface`] | the wireframe display | two sets of iso-parametric polylines | see there |

use si_math::algebra::{matrix_multiply, matrix_transpose, vector_normalize};
use si_math::types::Matrix;
use si_types::Ptr32;
use si_x87::{fmov, x, F80};

use crate::basis::{matrix_precision, spline_basis_matrix};
use crate::patch::{addf, gather, PatchSource, Surface, V0};
use crate::{held, mv, Vector};

/// One step along the rows: rows 3, 2, 1 each take the row before.
fn step_rows(g: &mut [Matrix; 3]) {
    for r in (1..4).rev() {
        for c in 0..4 {
            for m in g.iter_mut() {
                m[r][c] = addf(m[r][c], m[r - 1][c]);
            }
        }
    }
}

/// One step along a row of differences `[third, second, first, value]`.
fn step_cols(t: &mut [[f32; 4]; 3]) {
    for c in (1..4).rev() {
        for row in t.iter_mut() {
            row[c] = addf(row[c], row[c - 1]);
        }
    }
}

fn row3(g: &[Matrix; 3]) -> [[f32; 4]; 3] {
    [g[0][3].map(fmov), g[1][3].map(fmov), g[2][3].map(fmov)]
}

fn at3(t: &[[f32; 4]; 3]) -> Vector {
    Vector { x: fmov(t[0][3]), y: fmov(t[1][3]), z: fmov(t[2][3]) }
}

/// The grid of points [`patch_surface`] returns.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    /// `nu × nv` points, point (i, j) at `i × nv + j`.
    pub points: Vec<Vector>,
    pub nu: i16,
    pub nv: i16,
}

/// The patch as a grid of points [soft 0x0847f89c SI_patchSurface].
///
/// `nu = u_step × segments_u`, plus 1 when U is open (16-bit arithmetic), and likewise
/// `nv`. Cell (cu, cv) writes its points at `(u_step × cu + a, v_step × cv + b)` for
/// `a < u_step` and `b < v_step`, with one more row for the last cell of an open U and one
/// more column for the last cell of an open V: a cell's far edge is its neighbour's near
/// edge and is written by the neighbour, and a closed direction has no last edge (the grid
/// wraps).
///
/// Because each cell starts from its own corner and steps by differences rounded to float,
/// **neighbouring cells do not meet bit for bit** in general; the grid holds the
/// neighbour's values on a shared edge.
///
/// The two bases are written into one local matrix (see [`crate::patch::update_matrix`]:
/// an unknown V type reuses the U basis). `None` where the original writes through a null
/// pointer or outside its block: a point count that is zero or negative as a product of two
/// signed 16-bit numbers while there are cells to write.
pub fn patch_surface(s: &Surface) -> Option<Grid> {
    let (nu, nv) = grid_dims(&s.rec);
    let count = nu as i32 * nv as i32;
    let mut out = vec![V0; count.max(0) as usize];
    grid_cells(s, nv, &mut |index, p| {
        *out.get_mut(usize::try_from(index).ok()?)? = p;
        Some(())
    })?;
    Some(Grid { points: out, nu, nv })
}

/// `nu` and `nv` of [`patch_surface`]'s grid: `step × segments`, plus 1 for an open
/// direction, in 16 bits.
pub(crate) fn grid_dims(r: &si_types::geom::Patch) -> (i16, i16) {
    let ((nsu, _), (nsv, _)) = crate::patch::cells_of(r);
    let nu = r.u_step.wrapping_mul(nsu as u16).wrapping_add((r.u_closed == 0) as u16) as i16;
    let nv = r.v_step.wrapping_mul(nsv as u16).wrapping_add((r.v_closed == 0) as u16) as i16;
    (nu, nv)
}

/// The cells of [`patch_surface`], each point handed to `put` with its index in the
/// grid, in the original's order. `None` where the original would fault (a point that
/// cannot be read, or `put` refuses: a write outside the block).
pub(crate) fn grid_cells(s: &(impl PatchSource + ?Sized), nv: i16, put: &mut dyn FnMut(i32, Vector) -> Option<()>) -> Option<()> {
    let r = s.rec();
    let ((nsu, stu), (nsv, stv)) = s.cells();
    let mut b = [[0.0f32; 4]; 4];
    spline_basis_matrix(r.u_type, r.u_tension, &mut b);
    let du = matrix_multiply(&matrix_precision(r.u_step as i32), &b);
    spline_basis_matrix(r.v_type, r.v_tension, &mut b);
    let dvt = matrix_transpose(&matrix_multiply(&matrix_precision(r.v_step as i32), &b));
    let (us, vs) = (r.u_step as i32, r.v_step as i32);
    for cu in 0..nsu {
        for cv in 0..nsv {
            let g = gather(s, cu.wrapping_mul(stu), cv.wrapping_mul(stv), false)?;
            let mut g = g.map(|m| matrix_multiply(&matrix_multiply(&du, &m), &dvt));
            let rows = us + (cu == nsu - 1 && r.u_closed == 0) as i32;
            for a in 0..rows {
                let mut t = row3(&g);
                let mut index = vs.wrapping_mul(cv).wrapping_add(us.wrapping_mul(cu).wrapping_add(a).wrapping_mul(nv as i32));
                put(index, at3(&t))?;
                let cols = vs + (cv == nsv - 1 && r.v_closed == 0) as i32;
                for _ in 1..cols {
                    index = index.wrapping_add(1);
                    step_cols(&mut t);
                    put(index, at3(&t))?;
                }
                step_rows(&mut g);
            }
        }
    }
    Some(())
}

/// What [`compute_surface`] recomputes.
pub const ALL: i16 = 0;
/// Only the cells that the point whose index is at +0x1ac (the point being dragged) moves.
pub const ONE_POINT: i16 = 1;
/// Only the cells that a tagged point (bit 0 of its tag word) moves.
pub const TAGGED: i16 = 2;

/// Frees and rebuilds everything derived from the control points
/// [soft 0x0847cee4 SI_patchUpdateSurface].
///
/// - `surface0`: `u_step × segments_u + 1` points a curve (+0x1b4) by
///   `u_nb_curves × segments_v`, plus 1 when V is open, curves (+0x1b6), with `modif0`;
/// - `surface1`: `v_step × segments_v + 1` (+0x1bc) by `v_nb_curves × segments_u`, plus 1
///   when U is open (+0x1be), with `modif1`;
/// - the two arrays of the shading grid, `segments_u × segments_v × (u_step + 1) ×
///   (v_step + 1)` vectors each, cleared and **not computed** here (the flag at +0x1c0 is
///   cleared by [`compute_surface`], and [`computes_subdivision`] fills them on demand).
///
/// The four dimensions are computed in 16 bits. All blocks come from `SI_calloc`, which
/// returns null for a count of 0. Ends with [`compute_surface`]`(patch, 0)`.
pub fn update_surface(s: &mut Surface) {
    let ((nsu, _), (nsv, _)) = s.cells();
    let r = &mut s.rec;
    let block = |n: i64| -> Vec<Vector> { vec![V0; n.clamp(0, 1 << 26) as usize] };
    r.surface0_dims[0] = r.u_step.wrapping_mul(nsu as u16).wrapping_add(1);
    r.surface0_dims[1] = r.u_nb_curves.wrapping_mul(nsv as u16).wrapping_add((r.v_closed == 0) as u16);
    let n0 = r.surface0_dims[0] as i64 * r.surface0_dims[1] as i64;
    r.surface1_dims[0] = r.v_step.wrapping_mul(nsv as u16).wrapping_add(1);
    r.surface1_dims[1] = r.v_nb_curves.wrapping_mul(nsu as u16).wrapping_add((r.u_closed == 0) as u16);
    let n1 = r.surface1_dims[0] as i64 * r.surface1_dims[1] as i64;
    let n2 = nsu.wrapping_mul(nsv).wrapping_mul(r.u_step as i32 + 1).wrapping_mul(r.v_step as i32 + 1) as i64;
    r.surface0 = if n0 == 0 { Ptr32::NULL } else { held() };
    r.modif0 = if n0 == 0 { Ptr32::NULL } else { held() };
    r.surface1 = if n1 == 0 { Ptr32::NULL } else { held() };
    r.modif1 = if n1 == 0 { Ptr32::NULL } else { held() };
    r.subdiv0 = if n2 == 0 { Ptr32::NULL } else { held() };
    r.subdiv1 = if n2 == 0 { Ptr32::NULL } else { held() };
    s.surface0 = block(n0);
    s.modif0 = vec![0; s.surface0.len()];
    s.surface1 = block(n1);
    s.modif1 = vec![0; s.surface1.len()];
    s.subdiv0 = block(n2);
    s.subdiv1 = block(n2);
    compute_surface(s, ALL);
}

/// Computes the display curves [soft 0x0847c1d0 SI_patchComputeSurface].
///
/// Does nothing when bit 0 of the word at +0x1e4 is set. Otherwise:
///
/// 1. the shading grid is marked stale (+0x1c0 = 0);
/// 2. each capping flag of an open direction collapses its border row first
///    ([`force_capping`] with the first point, not the average): V bottom, V top, U bottom,
///    U top, in that order. Any capping flag at all then forces a full update;
/// 3. +0x1e0 records whether the update is partial; a partial update first clears both
///    `modif` arrays and then marks every point it writes with 1;
/// 4. per cell (U major): in a partial update the cell is skipped unless one of its 16
///    control points is the wanted one (a linear direction only looks at its first two
///    rows). The cell's V-running curves come from `matrices[0] × G × matrices[2]`:
///    `v_nb_curves` of them, one more on the last cell of an open U, each of `v_step + 1`
///    points written at `v_step × cv + (v_nb_curves × cu + a) × dims1[0]` onwards into
///    `surface1`. Then the U-running curves from `matrices[1] × Gᵀ × matrices[3]` into
///    `surface0` likewise. A cell's last point of a curve is overwritten by the next
///    cell's first.
///
/// In a partial update, when every wanted point of the cell is in its last row (row 3, or
/// row 1 of a linear direction) the cell's first curve is skipped, and when every one is in
/// its first row the extra last curve is: those curves do not depend on such a point.
///
/// Writes that would fall outside an array (the original has no check; the arrays must
/// come from [`update_surface`] with the same counts) are skipped by the port.
pub fn compute_surface(s: &mut Surface, mode: i16) {
    if s.rec.unk_1e4 & 1 != 0 {
        return;
    }
    let ((nsu, stu), (nsv, stv)) = s.cells();
    s.rec.unk_1c0 = 0;
    if s.rec.v_bottom != 0 && s.rec.v_closed == 0 {
        force_capping(s, 0, 0);
    }
    if s.rec.v_top != 0 && s.rec.v_closed == 0 {
        force_capping(s, 1, 0);
    }
    if s.rec.u_bottom != 0 && s.rec.u_closed == 0 {
        force_capping(s, 2, 0);
    }
    if s.rec.u_top != 0 && s.rec.u_closed == 0 {
        force_capping(s, 3, 0);
    }
    let mut mode = mode;
    if s.rec.u_bottom != 0 || s.rec.u_top != 0 || s.rec.v_bottom != 0 || s.rec.v_top != 0 {
        mode = ALL;
    }
    s.rec.unk_1e0 = (mode != 0) as i16;
    if mode != 0 {
        let n0 = s.rec.surface0_dims[0] as usize * s.rec.surface0_dims[1] as usize;
        s.modif0.iter_mut().take(n0).for_each(|m| *m = 0);
        let n1 = s.rec.surface1_dims[0] as usize * s.rec.surface1_dims[1] as usize;
        s.modif1.iter_mut().take(n1).for_each(|m| *m = 0);
    }
    let (nu, nv) = (s.rec.nb_u as i32, s.rec.nb_v as i32);
    let (us, vs) = (s.rec.u_step as i32, s.rec.v_step as i32);
    let (uc, vc) = (s.rec.u_nb_curves as i32, s.rec.v_nb_curves as i32);
    for cu in 0..nsu {
        for cv in 0..nsv {
            let mut compute = mode == 0;
            let (mut last_u, mut first_u, mut last_v, mut first_v) = (mode != 0, mode != 0, mode != 0, mode != 0);
            let mut g = [[[0.0f32; 4]; 4]; 3];
            let mut gt = [[[0.0f32; 4]; 4]; 3];
            for i in 0..4usize {
                for j in 0..4usize {
                    let row = cu.wrapping_mul(stu).wrapping_add(i as i32) % nu;
                    let col = cv.wrapping_mul(stv).wrapping_add(j as i32) % nv;
                    let index = col.wrapping_add(row.wrapping_mul(nv));
                    if (i < 2 || s.rec.u_type != 0) && (j < 2 || s.rec.v_type != 0) {
                        let wanted = (mode == TAGGED && s.tags.get(index as usize).is_some_and(|t| t & 1 != 0))
                            || (mode == ONE_POINT && s.rec.unk_1ac as u16 as i32 == index);
                        if wanted {
                            compute = true;
                            if i != (if s.rec.u_type != 0 { 3 } else { 1 }) {
                                last_u = false;
                            }
                            if i != 0 {
                                first_u = false;
                            }
                            if j != (if s.rec.v_type != 0 { 3 } else { 1 }) {
                                last_v = false;
                            }
                            if j != 0 {
                                first_v = false;
                            }
                        }
                    }
                    // outside the block the original reads other memory: zeros here
                    let p = s.point(index).unwrap_or([0.0; 4]);
                    for k in 0..3 {
                        g[k][i][j] = fmov(p[k]);
                        gt[k][j][i] = fmov(p[k]);
                    }
                }
            }
            if !compute {
                continue;
            }
            // the curves that run in V, at steps of u
            let (a, b) = (s.rec.matrices[0], s.rec.matrices[2]);
            let mut h = g.map(|m| matrix_multiply(&matrix_multiply(&a, &m), &b));
            let mut count = vc;
            if cu == nsu - 1 && s.rec.u_closed == 0 && !first_u {
                count += 1;
            }
            let mut k = 0;
            if last_u {
                k = 1;
                step_rows(&mut h);
            }
            let width = s.rec.surface1_dims[0] as i32;
            while k < count {
                let mut t = row3(&h);
                let mut index = vs.wrapping_mul(cv).wrapping_add(vc.wrapping_mul(cu).wrapping_add(k).wrapping_mul(width));
                put(&mut s.surface1, &mut s.modif1, index, &t, mode);
                for _ in 0..vs {
                    index = index.wrapping_add(1);
                    step_cols(&mut t);
                    put(&mut s.surface1, &mut s.modif1, index, &t, mode);
                }
                step_rows(&mut h);
                k += 1;
            }
            // the curves that run in U, at steps of v
            let (a, b) = (s.rec.matrices[1], s.rec.matrices[3]);
            let mut h = gt.map(|m| matrix_multiply(&matrix_multiply(&a, &m), &b));
            let mut count = uc;
            if cv == nsv - 1 && s.rec.v_closed == 0 && !first_v {
                count += 1;
            }
            let mut k = 0;
            if last_v {
                k = 1;
                step_rows(&mut h);
            }
            let width = s.rec.surface0_dims[0] as i32;
            while k < count {
                let mut t = row3(&h);
                let mut index = us.wrapping_mul(cu).wrapping_add(uc.wrapping_mul(cv).wrapping_add(k).wrapping_mul(width));
                put(&mut s.surface0, &mut s.modif0, index, &t, mode);
                for _ in 0..us {
                    index = index.wrapping_add(1);
                    step_cols(&mut t);
                    put(&mut s.surface0, &mut s.modif0, index, &t, mode);
                }
                step_rows(&mut h);
                k += 1;
            }
        }
    }
}

fn put(points: &mut [Vector], modif: &mut [i16], index: i32, t: &[[f32; 4]; 3], mode: i16) {
    if index < 0 {
        return;
    }
    if let Some(p) = points.get_mut(index as usize) {
        *p = at3(t);
    }
    if mode != 0 {
        if let Some(m) = modif.get_mut(index as usize) {
            *m = 1;
        }
    }
}

/// Collapses a border row of control points to one point and sets the row outside it so
/// that the surface ends there [soft 0x0848120c SI_patchForceCapping].
///
/// `which`: 0 the V bottom (the row j = 0, or j = 1 when the V type is a B-spline or
/// cardinal, whose first row is not on the surface), 1 the V top (j = nb_v − 1, or
/// nb_v − 2), 2 the U bottom, 3 the U top. Any other value leaves the original working on
/// uninitialised locals; the port does nothing.
///
/// 1. The pole is the row's first point, or with `average` not 0 the mean of the row
///    (summed in floats from 0, times `1 / count` kept in the register).
/// 2. Every point of the row becomes the pole (x, y, z; the fourth float stays).
/// 3. B-spline (type 2), with `in` the neighbouring row inside and `out` the row outside:
///    `out = (C − 4 × row) − in`, where
///    `C = float(float(4 × pole) + float((2 / count) × Σ in))`: the row outside mirrors the
///    row inside through the pole so that the surface passes through it. `row` is read
///    back from the grid (it is the pole by now); x keeps `C − 4 × row` in the register,
///    y and z round it to float first.
/// 4. Cardinal (type 3): the normal N of the polygon the row inside forms (Newell's sums,
///    each term rounded to float as it is added); when `N·N` is above 0.0001, each point of
///    the row outside becomes `float(2 × (pole + N × t)) − in` with
///    `t = (N · (in − pole)) / float(N·N)`.
///
/// `false`, with nothing changed, where the original would read or write outside the
/// block of points (a direction with too few rows for its type).
pub fn force_capping(s: &mut Surface, which: i16, average: i16) -> bool {
    let Some((kind, start, count, stride, inward)) = capping_rows(s, which) else { return false };
    if count == 0 {
        return true; // nothing is written; the pole it reads has no use
    }
    if !capping_in_block(s, which) {
        return false;
    }
    let at = |s: &Surface, i: i32| s.points[i as usize];
    let mut pole = [0.0f32; 3];
    if average != 0 {
        for k in 0..count {
            let p = at(s, start + k * stride);
            for c in 0..3 {
                pole[c] = addf(pole[c], p[c]);
            }
        }
        let inv = F80::ONE / F80::from_i32(count);
        pole = [(inv * pole[0]).to_f32(), (inv * pole[1]).to_f32(), (inv * pole[2]).to_f32()];
    } else {
        let p = at(s, start);
        pole = [fmov(p[0]), fmov(p[1]), fmov(p[2])];
    }
    for k in 0..count {
        let p = &mut s.points[(start + k * stride) as usize];
        (p[0], p[1], p[2]) = (pole[0], pole[1], pole[2]);
    }
    if kind == 2 {
        let mut sum = [0.0f32; 3];
        for k in 0..count {
            let p = at(s, start + inward + k * stride);
            for c in 0..3 {
                sum[c] = addf(sum[c], p[c]);
            }
        }
        let two = x(2.0f64) / F80::from_i32(count);
        let centre: [f32; 3] = core::array::from_fn(|c| {
            let four = (x(4.0f64) * pole[c]).to_f32();
            let mean = (two * sum[c]).to_f32();
            (x(four) + mean).to_f32()
        });
        for k in 0..count {
            let row = at(s, start + k * stride);
            let inner = at(s, start + inward + k * stride);
            let m = |c: usize| x((x(-4.0f64) * row[c]).to_f32()) + centre[c];
            let out = [(m(0) - inner[0]).to_f32(), (x(m(1).to_f32()) - inner[1]).to_f32(), (x(m(2).to_f32()) - inner[2]).to_f32()];
            let p = &mut s.points[(start - inward + k * stride) as usize];
            (p[0], p[1], p[2]) = (out[0], out[1], out[2]);
        }
    } else if kind == 3 {
        let mut prev = at(s, (count - 1) * stride + start + inward);
        let mut n = [0.0f32; 3];
        for k in 0..count {
            let cur = at(s, start + inward + k * stride);
            n[0] = (((x(prev[1]) - cur[1]) * (x(prev[2]) + cur[2])) + n[0]).to_f32();
            n[2] = (((x(cur[1]) + prev[1]) * (x(prev[0]) - cur[0])) + n[2]).to_f32();
            n[1] = (((x(prev[2]) - cur[2]) * (x(prev[0]) + cur[0])) + n[1]).to_f32();
            prev = cur;
        }
        let l = (x(n[0]) * n[0]).to_f32();
        let l = ((x(n[1]) * n[1]) + l).to_f32();
        let l80 = (x(n[2]) * n[2]) + l;
        let len2 = l80.to_f32();
        if !l80.gt(x(1.0e-4f64)) {
            return true;
        }
        for k in 0..count {
            let i = start + inward + k * stride;
            let p = at(s, i);
            let dx = x(p[0]) - pole[0];
            let dy = x(p[1]) - pole[1];
            let dz = x(p[2]) - pole[2];
            let d = (dz * n[2]) + ((x(n[1]) * dy) + (x(n[0]) * dx));
            let t = d / len2;
            let fx = (((x(n[0]) * t) + pole[0]) + ((x(n[0]) * t) + pole[0])).to_f32();
            let fy = (((x(n[1]) * t) + pole[1]) + ((x(n[1]) * t) + pole[1])).to_f32();
            let fz = ((x(pole[2]) + (x(n[2]) * t)) + (x(pole[2]) + (x(n[2]) * t))).to_f32();
            let out = [(x(fx) - p[0]).to_f32(), (x(fy) - p[1]).to_f32(), (x(fz) - p[2]).to_f32()];
            let q = &mut s.points[(i - 2 * inward) as usize];
            (q[0], q[1], q[2]) = (out[0], out[1], out[2]);
        }
    }
    true
}

/// The type, the first index of the border row, its length, the step from one of its
/// points to the next and the step to the neighbouring row inside, for [`force_capping`].
fn capping_rows(s: &Surface, which: i16) -> Option<(i32, i32, i32, i32, i32)> {
    let (nu, nv) = (s.rec.nb_u as i32, s.rec.nb_v as i32);
    Some(match which {
        0 => (s.rec.v_type, (s.rec.v_type as u32 > 1) as i32, nu, nv, 1),
        1 => (s.rec.v_type, if s.rec.v_type as u32 > 1 { nv - 2 } else { nv - 1 }, nu, nv, -1),
        2 => (s.rec.u_type, if s.rec.u_type as u32 > 1 { nv } else { 0 }, nv, 1, nv),
        3 => (s.rec.u_type, (if s.rec.u_type as u32 > 1 { nu - 2 } else { nu - 1 }).wrapping_mul(nv), nv, 1, -nv),
        _ => return None,
    })
}

/// Whether [`force_capping`] of this border stays inside the block of points (it reads
/// and writes the border row and, for a B-spline or cardinal direction, the row on either
/// side of it).
pub fn capping_in_block(s: &Surface, which: i16) -> bool {
    let Some((kind, start, count, stride, inward)) = capping_rows(s, which) else { return false };
    if count == 0 {
        return true;
    }
    let total = s.points.len() as i64;
    let inside = |i: i64| (0..total).contains(&i);
    let last = (count as i64 - 1) * stride as i64;
    let rows: &[i64] = if kind == 2 || kind == 3 { &[0, 1, -1] } else { &[0] };
    rows.iter().all(|off| {
        let first = start as i64 + off * inward as i64;
        inside(first) && inside(first + last)
    })
}

/// The forward-difference matrix of a **derivative** for `n` steps a segment
/// [soft 0x0847acb4 SI_matrixDerivativePrecision]. With d = float(1 / n), n a signed
/// 16-bit number:
///
/// ```text
/// |  0    0   0   0 |
/// | 6d²   0   0   0 |      2d is float(d + d); 3d² is float(d × (d × 3.0));
/// | 3d²  2d   0   0 |      6d² is float(3d² + 3d²) from the rounded 3d²
/// |  0    0   1   0 |
/// ```
///
/// Times `B × G` it gives, bottom up, the derivative at u = 0 and its first and second
/// forward differences (a cubic's derivative has no third).
pub fn matrix_derivative_precision(n: i16) -> Matrix {
    let d = (F80::ONE / F80::from_i32(n as i32)).to_f32();
    let mut m = [[0.0f32; 4]; 4];
    m[2][1] = (x(d) + d).to_f32();
    m[3][2] = 1.0;
    let t = (x(d) * (x(d) * 3.0f64)).to_f32();
    m[2][0] = t;
    m[1][0] = (x(t) + t).to_f32();
    m
}

/// Builds the shading grid: points and unit normals, `(u_step + 1) × (v_step + 1)` a cell
/// [soft 0x084817e8 SI_patchComputesSubdivision]. Returns at once when +0x1c0 is already
/// set, and sets it.
///
/// Both arrays are freed and allocated anew (cleared). The point of cell (cu, cv) at step
/// (a, b) is at `((cu × segments_v + cv) × (u_step + 1) + a) × (v_step + 1) + b`. Points
/// are stepped as in [`patch_surface`]; the two derivatives are stepped the same way from
/// `E′(u_step) × Bu × G × (E(v_step) × Bv)ᵀ` (in u) and `E(u_step) × Bu × G × (E′(v_step) ×
/// Bv)ᵀ` (in v), `E′` being [`matrix_derivative_precision`].
///
/// The normal is `∂v × ∂u`, each component a float, normalised by `SI_vectorNormalize`.
/// When its squared length (absolute) is not above the double 1e-6 the derivative that
/// vanished is replaced by a neighbour's, which is how the poles of a sphere or a capped
/// border get a normal:
///
/// - `|∂v|²` below 1e-6: ∂v of the next step in v (of the step before when b is not 0);
///   if that one is below 1e-6 too, ∂v of the next step in u (the step before when a is
///   not 0), whatever its length;
/// - otherwise ∂u is taken to be the one that vanished: ∂u of the next step in u (before,
///   when a is not 0); if below 1e-6, ∂u of the next step in v (before, when b is not 0).
///
/// "First step" is decided by two accumulated parameters being exactly 0, which they are
/// for a = 0 and b = 0 only. The neighbour is read from the cell's own table, with no
/// check: with a step count of 0 in a direction there is no neighbour and the original
/// reads past its table (zeros in the port).
///
/// `false` where the original divides by zero, reads outside the points, or read a
/// neighbour past its table (the grid is then built, with that neighbour as zeros).
pub fn computes_subdivision(s: &mut Surface) -> bool {
    if s.rec.unk_1c0 != 0 {
        return true;
    }
    s.rec.unk_1c0 = 1;
    let r = s.rec;
    let (us, vs) = (r.u_step as i32, r.v_step as i32);
    let per_cell = ((us + 1) * (vs + 1)) as usize;
    let mut tu = vec![V0; per_cell];
    let mut tv = vec![V0; per_cell];
    let mut b = [[0.0f32; 4]; 4];
    spline_basis_matrix(r.u_type, r.u_tension, &mut b);
    let du = matrix_multiply(&matrix_precision(us), &b);
    let ddu = matrix_multiply(&matrix_derivative_precision(r.u_step as i16), &b);
    spline_basis_matrix(r.v_type, r.v_tension, &mut b);
    let dvt = matrix_transpose(&matrix_multiply(&matrix_precision(vs), &b));
    let ddvt = matrix_transpose(&matrix_multiply(&matrix_derivative_precision(r.v_step as i16), &b));
    let ((nsu, stu), (nsv, stv)) = s.cells();
    let total = nsu.wrapping_mul(nsv).wrapping_mul(us + 1).wrapping_mul(vs + 1);
    let n = if total > 0 { total.min(1 << 26) as usize } else { 0 };
    s.subdiv0 = vec![V0; n];
    s.subdiv1 = vec![V0; n];
    s.rec.subdiv0 = if total == 0 { Ptr32::NULL } else { held() };
    s.rec.subdiv1 = s.rec.subdiv0;
    let overread = core::cell::Cell::new(false);
    let get = |t: &[Vector], i: i64| match usize::try_from(i).ok().and_then(|i| t.get(i)) {
        Some(v) => *v,
        None => {
            overread.set(true);
            V0
        }
    };
    let tiny = |v: Vector| (((x(v.x) * v.x) + (x(v.y) * v.y)) + (x(v.z) * v.z)).abs().lt(x(1.0e-6f64));
    let cross = |a: Vector, b: Vector| Vector {
        x: ((x(a.y) * b.z) - (x(a.z) * b.y)).to_f32(),
        y: ((x(a.z) * b.x) - (x(a.x) * b.z)).to_f32(),
        z: ((x(a.x) * b.y) - (x(a.y) * b.x)).to_f32(),
    };
    for cu in 0..nsu {
        for cv in 0..nsv {
            let Some(g) = gather(s, cu.wrapping_mul(stu), cv.wrapping_mul(stv), false) else { return false };
            let t = g.map(|m| matrix_multiply(&du, &m));
            let mut p = t.map(|m| matrix_multiply(&m, &dvt));
            let mut pv = t.map(|m| matrix_multiply(&m, &ddvt));
            let t = g.map(|m| matrix_multiply(&ddu, &m));
            let mut pu = t.map(|m| matrix_multiply(&m, &dvt));
            let base = ((vs + 1) * (us + 1)).wrapping_mul(cu.wrapping_mul(nsv).wrapping_add(cv)) as i64;
            let mut k = 0usize;
            for _ in 0..=us {
                let (mut rp, mut ru, mut rv) = (row3(&p), row3(&pu), row3(&pv));
                for bstep in 0..=vs {
                    if bstep != 0 {
                        step_cols(&mut rp);
                        step_cols(&mut ru);
                        step_cols(&mut rv);
                    }
                    if let Some(d) = s.subdiv0.get_mut((base + k as i64) as usize) {
                        *d = at3(&rp);
                    }
                    tu[k] = at3(&ru);
                    tv[k] = at3(&rv);
                    k += 1;
                }
                step_rows(&mut p);
                step_rows(&mut pu);
                step_rows(&mut pv);
            }
            let mut k = 0i64;
            for a in 0..=us {
                for bstep in 0..=vs {
                    let (dv, du_) = (get(&tv, k), get(&tu, k));
                    let mut nrm = cross(dv, du_);
                    let small = !(((x(nrm.x) * nrm.x) + (x(nrm.y) * nrm.y)) + (x(nrm.z) * nrm.z)).abs().gt(x(1.0e-6f64));
                    if small {
                        let along_v = |t: &[Vector]| if bstep == 0 { get(t, k + 1) } else { get(t, k - 1) };
                        let along_u = |t: &[Vector]| if a == 0 { get(t, k + vs as i64 + 1) } else { get(t, k - vs as i64 - 1) };
                        let dv_sq = ((x(dv.z) * dv.z) + ((x(dv.x) * dv.x) + (x(dv.y) * dv.y))).abs();
                        if dv_sq.lt(x(1.0e-6f64)) {
                            let mut w = along_v(&tv);
                            if tiny(w) {
                                w = along_u(&tv);
                            }
                            nrm = cross(w, du_);
                        } else {
                            let mut w = along_u(&tu);
                            if tiny(w) {
                                w = along_v(&tu);
                            }
                            nrm = cross(dv, w);
                        }
                    }
                    let mut v = mv(nrm);
                    vector_normalize(&mut v);
                    if let Some(d) = s.subdiv1.get_mut((base + k) as usize) {
                        *d = Vector { x: v.x, y: v.y, z: v.z };
                    }
                    k += 1;
                }
            }
        }
    }
    !overread.get()
}

/// What [`convert_to_mesh`] produces: the pieces of the `SI_Mesh` the original fills in
/// before it hands the mesh to the mesh package.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchMesh {
    /// The positions of the vertices: the grid of [`patch_surface`], U major.
    pub vertices: Vec<Vector>,
    /// `nu`, `nv` of the grid.
    pub dims: (i16, i16),
    /// Quadrilaterals, four vertex indices each, in the order they are given to
    /// `SI_polygonInitialize`.
    pub polygons: Vec<[i32; 4]>,
    /// Texture coordinates of each polygon's four corners, when the model has a texture.
    pub uvs: Option<Vec<[[f32; 2]; 4]>>,
}

/// What the mesh conversion asks of the model and mesh packages.
pub trait MeshHost {
    /// `SI_mdlGetMatAndTxt2D(model, 4)` returned a list (which the original frees at once
    /// with `SI_genListDestroy(…, SI_matAndTxt2DDestroy)`): the model has a 2D texture, and
    /// the polygons get texture coordinates.
    fn has_texture(&mut self) -> bool {
        false
    }
    /// The global `SI_nbPolMax` (0x086d3920): the largest polygon count allowed.
    fn nb_pol_max(&self) -> i32 {
        i32::MAX
    }
    /// The tail of the conversion on the finished mesh: `SI_meshUpdate(mesh)`,
    /// `SI_meshInitVertexColors(mesh)`, `SI_newMeshCleanup(mesh, 0.001, 0.0, 1, 0, 1)`
    /// (the tolerance is the float 0x3a83126f).
    fn finish(&mut self, _mesh: &mut PatchMesh) {}
}

/// A host without a texture and without a limit.
pub struct NoMeshHost;
impl MeshHost for NoMeshHost {}

/// The steps of the conversion's texture coordinates for `qu × qv` quadrilaterals:
/// `float(1 / qu)` and `float(1 / qv)`, each quotient rounded once.
pub fn uv_steps(qu: i32, qv: i32) -> (f32, f32) {
    ((F80::ONE / F80::from_i32(qu)).to_f32(), (F80::ONE / F80::from_i32(qv)).to_f32())
}

/// The texture coordinates of quadrilateral (i, j) of the conversion, corner by corner:
/// `(1 − i fu, j fv)`, `(1 − i fu, (j + 1) fv)`, `(1 − (i + 1) fu, (j + 1) fv)`,
/// `(1 − (i + 1) fu, j fv)`, each product formed in the register from the integer and
/// rounded once with the subtraction.
pub fn quad_uv(i: i32, j: i32, fu: f32, fv: f32) -> [[f32; 2]; 4] {
    let u0 = (F80::ONE - (F80::from_i32(i) * fu)).to_f32();
    let u1 = (F80::ONE - (F80::from_i32(i + 1) * fu)).to_f32();
    let v0 = (F80::from_i32(j) * fv).to_f32();
    let v1 = (F80::from_i32(j + 1) * fv).to_f32();
    [[u0, v0], [u0, v1], [u1, v1], [u1, v0]]
}

/// The patch as a mesh of quadrilaterals [soft 0x0847b69c SI_patchConvertToMesh], with
/// the texture coordinates of [soft 0x084868c8 patchToMeshSetPolygonUV].
///
/// Vertices are the grid of [`patch_surface`] (`nu × nv`, vertex (i, j) = `i × nv + j`).
/// With `qu = u_step × segments_u` and `qv = u_step × segments_v`, polygon `i × qv + j`
/// (i < qu outer, j < qv inner) has the corners, in this order,
///
/// ```text
/// (i, j)   (i, (j + 1) mod nv)   ((i + 1) mod nu, (j + 1) mod nv)   ((i + 1) mod nu, j)
/// ```
///
/// and, when the model has a texture, the coordinates `(1 − i fu, j fv)`,
/// `(1 − i fu, (j + 1) fv)`, `(1 − (i + 1) fu, (j + 1) fv)`, `(1 − (i + 1) fu, j fv)` with
/// `fu = float(1 / qu)`, `fv = float(1 / qv)`, each product formed in the register from the
/// integer and rounded once.
///
/// `None` is the original's null result: no polygon (`qu × qv ≤ 0`) or more than
/// `SI_nbPolMax` (the mesh is then disposed of; the texture query has already been made),
/// and the cases [`patch_surface`] refuses. The original allocates the `SI_Mesh`, its
/// vertices (20-byte records) and polygons (24-byte records with a node array of 36 bytes a
/// corner) through the mesh package's allocators; those belong to `si-model`, and the port
/// returns the data that goes into them.
pub fn convert_to_mesh(s: &Surface, host: &mut dyn MeshHost) -> Option<PatchMesh> {
    let ((nsu, _), (nsv, _)) = s.cells();
    let grid = patch_surface(s)?;
    let (nu, nv) = (grid.nu as i32, grid.nv as i32);
    let textured = host.has_texture();
    let qu = (s.rec.u_step as i32).wrapping_mul(nsu);
    let qv = (s.rec.v_step as i32).wrapping_mul(nsv);
    let count = qu.wrapping_mul(qv);
    if count <= 0 || count > host.nb_pol_max() {
        return None;
    }
    let (fu, fv) = uv_steps(qu, qv);
    let mut polygons = Vec::new();
    let mut uvs = Vec::new();
    for i in 0..qu {
        for j in 0..qv {
            if nu == 0 || nv == 0 {
                return None; // the original divides by zero
            }
            let i1 = (i + 1) % nu;
            let j1 = (j + 1) % nv;
            polygons.push([nv * i + j, i * nv + j1, i1 * nv + j1, j + i1 * nv]);
            if textured {
                uvs.push(quad_uv(i, j, fu, fv));
            }
        }
    }
    let mut mesh = PatchMesh { vertices: grid.points, dims: (grid.nu, grid.nv), polygons, uvs: textured.then_some(uvs) };
    host.finish(&mut mesh);
    Some(mesh)
}
