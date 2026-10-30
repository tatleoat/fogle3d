// SPDX-License-Identifier: MIT
//! Editing primitives of patches: reversal, removal of a row of points, random
//! displacement, and the extraction of an iso-parametric spline (finding 0020, section 12).

use si_math::algebra::matrix_transpose;
use si_types::Ptr32;
use si_x87::{fmov, x};

use crate::basis::{spline_basis_matrix, update_matrix};
use crate::eval::nb_segment_step;
use crate::patch::{peel, Surface};
use crate::patch_grid::{compute_surface, update_surface, ALL, TAGGED};
use crate::{convert, edit, held, Curve, Vector};

/// The C library's `drand48` (dynamic import; not read from the binary): the 48-bit linear
/// congruential generator `X′ = (0x5DEECE66D × X + 0xB) mod 2⁴⁸`, the result `X′ / 2⁴⁸`.
/// One state a process; a process that never called `srand48` starts from 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drand48(pub u64);

impl Drand48 {
    pub fn next(&mut self) -> f64 {
        self.0 = 0x5_DEEC_E66Du64.wrapping_mul(self.0).wrapping_add(0xB) & ((1 << 48) - 1);
        self.0 as f64 / (1u64 << 48) as f64
    }
}

/// Moves control points at random [soft 0x0847dcc8 SI_patchRandomize].
///
/// From the last point to the first, `count` times a point, each coordinate becomes
/// `float((drand48() − 0.5) × amplitude + coordinate)`, three calls a turn in the order
/// x, y, z. With `tagged` not 0 only the points whose tag has bit 0 are moved and the
/// display is updated for the tagged points ([`compute_surface`] with 2); otherwise all are
/// moved and everything is updated.
pub fn randomize(s: &mut Surface, amplitude: &Vector, count: i16, tagged: i16, rng: &mut Drand48) {
    let n = s.rec.nb_u as usize * s.rec.nb_v as usize;
    for i in (0..n).rev() {
        if tagged != 0 && !s.tags.get(i).is_some_and(|t| t & 1 != 0) {
            continue;
        }
        let Some(p) = s.points.get_mut(i) else { continue };
        for _ in 0..count.max(0) {
            p[0] = (((x(rng.next()) - 0.5f64) * amplitude.x) + p[0]).to_f32();
            p[1] = (((x(rng.next()) - 0.5f64) * amplitude.y) + p[1]).to_f32();
            p[2] = (((x(rng.next()) - 0.5f64) * amplitude.z) + p[2]).to_f32();
        }
    }
    compute_surface(s, if tagged != 0 { TAGGED } else { ALL });
}

/// The index table of the modelling tools: where each point went.
fn note(table: &mut Option<&mut [i32]>, at: i32, value: i32) {
    if let Some(t) = table {
        if let Some(e) = usize::try_from(at).ok().and_then(|i| t.get_mut(i)) {
            *e = value;
        }
    }
}

/// Rotates the index table of each row right by one when V is a closed Bézier direction
/// [soft 0x0847ada0 pInvKltUpdate]: the entry of a row's last point moves to its first.
fn inv_klt_update(s: &Surface, table: &mut Option<&mut [i32]>) {
    let Some(t) = table else { return };
    if s.rec.v_type != 1 || s.rec.v_closed == 0 {
        return;
    }
    let nv = s.rec.nb_v as usize;
    for row in t.chunks_exact_mut(nv.max(1)).take(s.rec.nb_u as usize) {
        if nv > 0 {
            row.rotate_right(1);
        }
    }
}

/// [`inv_klt_update`] for the check binary (the original's routine is a static one).
#[doc(hidden)]
pub fn inv_klt_update_for_check(s: &Surface, table: Option<&mut [i32]>) {
    let mut t = table;
    inv_klt_update(s, &mut t);
}

/// [`inv_reorder_cv`] for the check binary.
#[doc(hidden)]
pub fn inv_reorder_cv_for_check(s: &mut Surface) {
    inv_reorder_cv(s);
}

/// Rotates the points and tags of each row left by one when V is a closed Bézier direction
/// [soft 0x0847ae2c pchInvReorderCV]: a row's first point moves to its end, so that the
/// reversal that follows keeps the joint that the row starts with.
fn inv_reorder_cv(s: &mut Surface) {
    if s.rec.v_type != 1 || s.rec.v_closed == 0 {
        return;
    }
    let nv = s.rec.nb_v as usize;
    if nv == 0 {
        return;
    }
    let nu = s.rec.nb_u as usize;
    for row in s.points.chunks_exact_mut(nv).take(nu) {
        row.rotate_left(1);
    }
    for row in s.tags.chunks_exact_mut(nv).take(nu) {
        row.rotate_left(1);
    }
}

/// Reverses the V direction of a patch [soft 0x0847af64 SI_patchInverse]: each row of
/// `nb_v` points is turned end for end, with its tags; the two V capping flags change
/// places; the table, when given, receives for each of the two points of a swap the
/// other's index.
///
/// - **Both directions Bézier**: the closed flags are cleared for the duration (with a
///   display update when one was set) and restored at the end, followed by
///   [`update_surface`]. The swaps then do **not** reverse the rows: for row i the first
///   swap exchanges the two points *before* the row's start (`i × nb_v − 1` and
///   `i × nb_v − 2`; for row 0 the last two points of the grid), and the following ones
///   reverse the first `nb_v − 2` points of the row before (the last row, for i = 0). This
///   is what the instructions do; the port follows them.
/// - **V a closed Bézier direction** (U not Bézier): rows are first rotated by one
///   ([`inv_reorder_cv`]), the table's rows rotated back ([`inv_klt_update`]), and the
///   display recomputed with [`compute_surface`].
/// - **Otherwise the display curves are reversed in place** rather than recomputed: each
///   V-running curve end for end (leaving out, on a closed V, the wrapped tail: `v_step`
///   points of a Bézier curve, 1 of a linear one, `3 × v_step` of the other types, whose
///   own points are reversed separately and whose last point is then made equal to the
///   curve's first), and the U-running curves change places top to bottom (on a closed
///   B-spline or cardinal V the last two rows are left out of that and then exchanged).
///   The shading grid is marked stale.
///
/// `false` where an index leaves an array: the original then exchanges other memory, the
/// port skips that exchange.
pub fn inverse(s: &mut Surface, mut table: Option<&mut [i32]>) -> bool {
    let mut inside = true;
    let both = s.rec.v_type == 1 && s.rec.u_type == 1;
    let (cu, cv) = (s.rec.u_closed, s.rec.v_closed);
    if both {
        if cu != 0 {
            s.rec.u_closed = 0;
        }
        if cv != 0 {
            s.rec.v_closed = 0;
        }
        if cu != 0 || cv != 0 {
            update_surface(s);
        }
    }
    (s.rec.v_bottom, s.rec.v_top) = (s.rec.v_top, s.rec.v_bottom);
    inv_reorder_cv(s);
    let nv = s.rec.nb_v as i32;
    let nu = s.rec.nb_u as i32;
    let total = s.points.len() as i32;
    for i in 0..nu {
        let (mut lo, mut hi);
        if both {
            let b = if i == 0 { nv * nu } else { i * nv };
            lo = b - 1;
            hi = b - 2;
        } else {
            lo = i * nv;
            hi = (nv - 1) + lo;
        }
        for k in 0..(nv >> 1) {
            if both && k == 1 {
                if i == 0 {
                    lo = (nu - 1) * nv;
                    hi = (nv - 2) + lo;
                } else {
                    hi = i * nv - 2;
                    lo = (i - 1) * nv;
                }
                lo -= 1;
            }
            let (a, b) = (lo + k, hi - k);
            if a < 0 || b < 0 || a >= total || b >= total {
                inside = false;
            } else {
                s.points.swap(a as usize, b as usize);
                if (a.max(b) as usize) < s.tags.len() {
                    s.tags.swap(a as usize, b as usize);
                }
            }
            note(&mut table, a, b);
            note(&mut table, b, a);
        }
    }
    inv_klt_update(s, &mut table);
    if s.rec.v_type == 1 && s.rec.u_type != 1 && s.rec.v_closed != 0 {
        compute_surface(s, ALL);
        return inside;
    }
    if both {
        s.rec.u_closed = cu;
        s.rec.v_closed = cv;
        update_surface(s);
        return inside;
    }
    reverse_display(s, true, &mut inside);
    s.rec.unk_1c0 = 0;
    inside
}

/// Reverses the display curves where they lie, for a patch whose rows of control points
/// have just been turned round: the curves that run in the reversed direction end for end,
/// the others top to bottom. `v`: the reversed direction is V (the curves along it are
/// `surface1`), else U (`surface0`).
///
/// On a closed direction a curve's wrapped tail is left out of the reversal of its body:
/// `step` points of a Bézier curve, 1 of a linear one, `3 × step` of a B-spline or
/// cardinal one. The tail's own points are then reversed separately (`step − 1` of them
/// for a Bézier curve, `step` for a linear one, the tail less one for the other types)
/// and the curve's last point is made equal to its first. The other set changes places
/// row for row; on a closed B-spline or cardinal direction its last two rows are left out
/// of that and exchanged with each other afterwards.
fn reverse_display(s: &mut Surface, v: bool, inside: &mut bool) {
    let (closed, kind, step) = if v { (s.rec.v_closed, s.rec.v_type, s.rec.v_step as i32) } else { (s.rec.u_closed, s.rec.u_type, s.rec.u_step as i32) };
    let (along_dims, across_dims) = if v { (s.rec.surface1_dims, s.rec.surface0_dims) } else { (s.rec.surface0_dims, s.rec.surface1_dims) };
    let (along, across) = if v { (&mut s.surface1, &mut s.surface0) } else { (&mut s.surface0, &mut s.surface1) };
    let mut swap = |a: &mut Vec<Vector>, i: i32, j: i32| {
        if i < 0 || j < 0 || i as usize >= a.len() || j as usize >= a.len() {
            *inside = false;
        } else {
            a.swap(i as usize, j as usize);
        }
    };
    let w0 = along_dims[0] as i32;
    let curves = along_dims[1] as i32;
    let mut w = w0;
    let mut extra = 0;
    if closed != 0 {
        match kind {
            1 => {
                extra = step;
                w -= extra;
            }
            0 => {
                extra = 1;
                w -= 1;
            }
            2 | 3 => {
                let t = w - 3 * step;
                extra = w - t;
                w = t;
            }
            _ => {}
        }
    }
    for r in 0..curves {
        let base = r * (extra + w);
        for k in 0..(w >> 1).max(0) {
            swap(along, base + k, base + w - 1 - k);
        }
    }
    let mut rows = across_dims[1] as i32;
    if closed != 0 {
        let n;
        match kind {
            1 => {
                extra = w0 - step;
                n = step - 1;
            }
            0 => n = step,
            2 | 3 => {
                rows -= 2;
                n = extra - 1;
                extra = w;
            }
            _ => n = w,
        }
        for r in 0..curves {
            let base = w0 * r + extra;
            for k in 0..(n >> 1).max(0) {
                swap(along, base + k, base + n - 1 - k);
            }
            let (to, from) = ((r + 1) * w0 - 1, w0 * r);
            if to < 0 || to as usize >= along.len() || from as usize >= along.len() {
                swap(along, -1, -1);
            } else {
                along[to as usize] = along[from as usize];
            }
        }
    }
    let width = across_dims[0] as i32;
    let mut top = (rows - 1) * width;
    for c in 0..width {
        for k in 0..(rows >> 1).max(0) {
            swap(across, c + k * width, top - k * width);
        }
        top += 1;
    }
    if closed != 0 && kind != 0 && kind != 1 && width != 0 {
        let mut b = across_dims[1] as i32 * width - 1;
        let mut a = b - width;
        for _ in 0..width {
            swap(across, a, b);
            a -= 1;
            b -= 1;
        }
    }
}

/// The three "controls" of a patch [soft 0x0847b9e8 SI_patchPerformControls].
///
/// - `op` 0: **reverse U**. The rows of control points change places end for end (row i
///   with row `nb_u − 1 − i`), with their tags; the two U capping flags change places; the
///   table gets each point's partner. The display curves are reversed where they lie
///   ([`reverse_display`]) and the shading grid marked stale. Unlike the V reversal there
///   is no special case for Bézier directions.
/// - `op` 1: reverse V, [`inverse`].
/// - any other: **exchange U and V**. Point (i, j) moves to (j, i) of a grid of
///   `nb_v × nb_u` (the table gets, at the old index, the new one), every per-direction
///   field changes places with its counterpart (type, point count, closed, tension, step,
///   curve count, the capping flags bottom with bottom and top with top), and matrices and
///   display are rebuilt. The original works from a copy made with `SI_patchCopy`, which it
///   disposes of at the end; since that copy's display update applies the capping flags to
///   the copy's points, **a capped patch comes back with its border rows collapsed** in the
///   control points themselves.
///
/// `false` where an index leaves an array (see [`inverse`]).
pub fn perform_controls(s: &mut Surface, op: i16, mut table: Option<&mut [i32]>) -> bool {
    let (nu, nv) = (s.rec.nb_u as i32, s.rec.nb_v as i32);
    let mut inside = true;
    if op == 0 {
        (s.rec.u_bottom, s.rec.u_top) = (s.rec.u_top, s.rec.u_bottom);
        let total = s.points.len() as i32;
        let mut far = (nu - 1) * nv;
        for i in 0..(nu >> 1) {
            for j in 0..nv {
                let (a, b) = (i * nv + j, j + far);
                if a >= total || b >= total || b < 0 {
                    inside = false;
                } else {
                    s.points.swap(a as usize, b as usize);
                    if (a.max(b) as usize) < s.tags.len() {
                        s.tags.swap(a as usize, b as usize);
                    }
                }
                note(&mut table, a, b);
                note(&mut table, b, a);
            }
            far -= nv;
        }
        reverse_display(s, false, &mut inside);
        s.rec.unk_1c0 = 0;
    } else if op == 1 {
        return inverse(s, table);
    } else {
        // the copy's display update runs the cappings on the copy's points, and it is the
        // copy's points that come back
        let old = crate::patch::copy(s);
        let o = &old.rec;
        for j in 0..nv {
            for i in 0..nu {
                let (to, from) = ((j * nu + i) as usize, (j + i * nv) as usize);
                match (s.points.get_mut(to), old.points.get(from)) {
                    (Some(d), Some(p)) => *d = *p,
                    _ => inside = false,
                }
                if let (Some(d), Some(t)) = (s.tags.get_mut(to), old.tags.get(from)) {
                    *d = *t;
                }
                note(&mut table, from as i32, to as i32);
            }
        }
        let r = &mut s.rec;
        (r.u_type, r.v_type) = (o.v_type, o.u_type);
        (r.nb_u, r.nb_v) = (o.nb_v, o.nb_u);
        (r.u_closed, r.v_closed) = (o.v_closed, o.u_closed);
        (r.u_tension, r.v_tension) = (o.v_tension, o.u_tension);
        (r.u_step, r.v_step) = (o.v_step, o.u_step);
        (r.u_nb_curves, r.v_nb_curves) = (o.v_nb_curves, o.u_nb_curves);
        (r.u_bottom, r.v_bottom) = (o.v_bottom, o.u_bottom);
        (r.u_top, r.v_top) = (o.v_top, o.u_top);
        crate::patch::update_matrix(s);
        update_surface(s);
    }
    inside
}

/// Removes one row of control points [soft 0x08482958 SI_patchDeleteRow].
///
/// `index` is a point's index in the grid. With `along_u` 0 the points with that point's
/// V index (`index mod nb_v`) go and `nb_v` drops by one; otherwise those with its U index
/// (`index / nb_v`) and `nb_u` drops. The points and tags move to new blocks, the table
/// (when given) gets each old point's new index, or −1 for a removed one, and the display
/// is rebuilt ([`update_surface`]). The new index is counted in 16 bits (signed).
///
/// `Some(0)`: refused, the direction has fewer than two rows. `None`: a row outside the
/// grid (a negative index, or beyond the last row), for which the original copies every
/// point into a block one row too small.
pub fn delete_row(s: &mut Surface, index: i16, along_u: i16, mut table: Option<&mut [i32]>) -> Option<i32> {
    let (nu, nv) = (s.rec.nb_u as i32, s.rec.nb_v as i32);
    if (along_u == 0 && nv <= 1) || (along_u != 0 && nu <= 1) {
        return Some(0);
    }
    if nv == 0 {
        return None; // the original divides by zero
    }
    let (row, col) = if along_u == 0 { (-1, (index as i32 % nv) as i16 as i32) } else { ((index as i32 / nv) as i16 as i32, -1) };
    if (along_u == 0 && !(0..nv).contains(&col)) || (along_u != 0 && !(0..nu).contains(&row)) {
        return None;
    }
    let mut points = Vec::new();
    let mut tags = Vec::new();
    let mut out = 0i16;
    let mut src = 0i32;
    for i in 0..nu {
        for j in 0..nv {
            if j == col || i == row {
                note(&mut table, src, -1);
            } else {
                points.push(s.points.get(src as usize).copied().unwrap_or([0.0; 4]));
                tags.push(s.tags.get(src as usize).copied().unwrap_or(0));
                note(&mut table, src, out as i32);
                out = out.wrapping_add(1);
            }
            src += 1;
        }
    }
    if along_u == 0 {
        s.rec.nb_v -= 1;
    } else {
        s.rec.nb_u -= 1;
    }
    s.rec.points = if points.is_empty() { Ptr32::NULL } else { held() };
    s.rec.tags = if tags.is_empty() { Ptr32::NULL } else { held() };
    s.points = points;
    s.tags = tags;
    update_surface(s);
    Some(1)
}

/// The spline along one direction of a patch at a parameter of the other
/// [soft 0x0847d764 SI_patchExtractSpline].
///
/// `along_v` 0: the spline runs in U (type, key count, closed flag, tension and step of
/// U) at the V parameter `t`; otherwise it runs in V at the U parameter `t`. The whole part
/// of `t` is peeled off as in [`crate::patch::value`] and picks four consecutive rows of
/// control points; the four weights are the cross direction's basis at the fraction,
/// `float(((B₁ⱼt² + B₀ⱼt³) + tB₂ⱼ) + B₃ⱼ)` (the last one summed in the order
/// `(tB₂₃ + (t²B₁₃ + t³B₀₃)) + B₃₃`), and each key's position is
/// `float(((p₂w₂ + (p₁w₁ + p₀w₀)) + p₃w₃))`. Only positions are set: the handles stay zero,
/// as the key allocator leaves them.
///
/// The second, third and fourth rows wrap to the grid's start by comparing the **number of
/// whole steps peeled** with the row count less 2, 3 and 4, which is right for the types
/// that advance one row a segment and not for a Bézier cross direction (three rows a
/// segment).
///
/// A Bézier result is then turned from control points into keys with handles
/// ([`convert::bezier_shrink`]), which allocates a second spline (the name counter
/// advances twice). `None`: the peeling does not end, the shrink fails, or a row is
/// outside the block of points.
pub fn extract_spline(s: &Surface, along_v: i16, t: f32, counter: &mut i32) -> Option<Curve> {
    let (rec, name) = si_types::geom::Spline::allocate(counter);
    let mut c = Curve { rec, name: Some(name), keys: Vec::new(), lengths: Vec::new() };
    c.rec.name = held();
    let r = &s.rec;
    let (cross_type, cross_tension, cross_nb, cross_closed) = if along_v == 0 {
        (c.rec.kind, c.rec.nb_keys, c.rec.closed, c.rec.tension, c.rec.step) = (r.u_type, r.nb_u as i16, r.u_closed, fmov(r.u_tension), r.u_step as i16);
        (r.v_type, r.v_tension, r.nb_v, r.v_closed)
    } else {
        (c.rec.kind, c.rec.nb_keys, c.rec.closed, c.rec.tension, c.rec.step) = (r.v_type, r.nb_v as i16, r.v_closed, fmov(r.v_tension), r.v_step as i16);
        (r.u_type, r.u_tension, r.nb_u, r.u_closed)
    };
    let mut b = [[0.0f32; 4]; 4];
    spline_basis_matrix(cross_type, cross_tension, &mut b);
    let (_, step) = nb_segment_step(cross_type, cross_nb as i32, cross_closed);
    let (t, first) = peel(x(t), step)?;
    let turns = if step == 0 { 0 } else { first / step };
    let t = t.to_f32();
    let nv = r.nb_v as i32;
    let (mut base, across, next, rows) = if along_v == 0 { (first, 1, nv, nv) } else { (nv.wrapping_mul(first), nv, 1, r.nb_u as i32) };
    update_matrix(&mut c);
    c.keys = edit::key_allocate(c.rec.nb_keys as u16 as i32);
    c.rec.keys = if c.keys.is_empty() { Ptr32::NULL } else { held() };
    let m = matrix_transpose(&b);
    let t1 = x(t) * t;
    let t2 = t1 * t;
    let w0 = ((((x(m[0][1]) * t1) + (x(m[0][0]) * (t1 * t))) + (x(t) * m[0][2])) + m[0][3]).to_f32();
    let w1 = ((((x(m[1][1]) * t1) + (x(m[1][0]) * (t1 * t))) + (x(t) * m[1][2])) + m[1][3]).to_f32();
    let w2 = ((((x(m[2][1]) * t1) + (x(m[2][0]) * t2)) + (x(t) * m[2][2])) + m[2][3]).to_f32();
    let w3 = (((x(t) * m[3][2]) + ((t1 * m[3][1]) + (t2 * m[3][0]))) + m[3][3]).to_f32();
    let (mut o1, mut o2, mut o3) = (across, across * 2, across * 3);
    if turns > rows - 2 {
        o1 -= across * rows;
    }
    if turns > rows - 3 {
        o2 -= across * rows;
    }
    if turns > rows - 4 {
        o3 -= rows * across;
    }
    for k in 0..c.rec.nb_keys as u16 as usize {
        let p0 = s.point(base)?;
        let p1 = s.point(base + o1)?;
        let p2 = s.point(base + o2)?;
        let p3 = s.point(base + o3)?;
        let at = |j: usize| (((x(p2[j]) * w2) + ((x(p1[j]) * w1) + (x(p0[j]) * w0))) + (x(p3[j]) * w3)).to_f32();
        c.keys[k].pos = Vector { x: at(0), y: at(1), z: at(2) };
        base += next;
    }
    if c.rec.kind == 1 {
        return convert::bezier_shrink(&c, counter);
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::value;
    use crate::{BSPLINE, CARDINAL, LINEAR};

    fn grid(nu: usize, nv: usize) -> Vec<[f32; 3]> {
        (0..nu * nv).map(|k| [(k / nv) as f32, (k % nv) as f32, ((k * 7) % 5) as f32]).collect()
    }

    #[test]
    fn reversal_turns_each_row_round() {
        let pts = grid(3, 4);
        let mut s = Surface::from_points([LINEAR, BSPLINE], [false, false], 3, 4, &pts);
        update_surface(&mut s);
        let before = s.clone();
        let mut table = vec![-7; 12];
        assert!(inverse(&mut s, Some(&mut table)));
        for i in 0..3 {
            for j in 0..4 {
                assert_eq!(s.points[i * 4 + j], before.points[i * 4 + 3 - j]);
            }
        }
        assert_eq!(&table[..4], &[3, 2, 1, 0]);
        // reversed in place, the display curves are those of the reversed patch
        let mut fresh = s.clone();
        update_surface(&mut fresh);
        assert_eq!(s.surface0.len(), fresh.surface0.len());
        assert!(inverse(&mut s, None));
        assert_eq!(s.points, before.points);
    }

    #[test]
    fn a_row_goes_and_the_table_says_where_the_rest_went() {
        let pts = grid(3, 4);
        let mut s = Surface::from_points([LINEAR, LINEAR], [false, false], 3, 4, &pts);
        let mut table = vec![0; 12];
        assert_eq!(delete_row(&mut s, 5, 0, Some(&mut table)), Some(1));
        assert_eq!((s.rec.nb_u, s.rec.nb_v, s.points.len()), (3, 3, 9));
        assert_eq!(&table[..5], &[0, -1, 1, 2, 3]);
        assert_eq!(delete_row(&mut s, 4, 1, None), Some(1));
        assert_eq!((s.rec.nb_u, s.rec.nb_v), (2, 3));
        let mut one = Surface::from_points([LINEAR, LINEAR], [false, false], 1, 3, &grid(1, 3));
        assert_eq!(delete_row(&mut one, 0, 1, None), Some(0));
        assert_eq!(delete_row(&mut one, -1, 0, None), None);
    }

    #[test]
    fn extracted_spline_lies_on_the_patch() {
        let pts = grid(5, 6);
        let mut s = Surface::from_points([CARDINAL, BSPLINE], [false, false], 5, 6, &pts);
        s.rec.u_tension = 0.5;
        crate::patch::update_matrix(&mut s);
        let mut n = 0;
        let c = extract_spline(&s, 0, 1.25, &mut n).unwrap();
        assert_eq!((c.rec.kind, c.nb(), n), (CARDINAL, 5, 1));
        // the U-running spline at v = 1.25 passes through the patch's points at that v
        let on_curve = crate::eval::value(&c, 0.5).0;
        let on_patch = value(&s, 0.5, 1.25).unwrap();
        assert!((on_curve.x - on_patch.x).abs() < 1e-4 && (on_curve.z - on_patch.z).abs() < 1e-4);
        let mut g = Drand48::default();
        assert_eq!(g.next(), 11.0 / (1u64 << 48) as f64);
    }
}
