// SPDX-License-Identifier: MIT
//! Conversions between the spline types and the subdivisions they rest on: stepping a
//! curve into a polyline (regular steps, or as few points as a tolerance allows), turning any
//! type into Bézier keys, the two Bézier layouts (keys with handles, and plain control
//! points three to a segment), removal of superfluous points, and the split of one segment.

use si_math::algebra::{matrix_inverse, matrix_multiply, points_colinear, vector_normalize};
use si_math::types::Matrix;
use si_types::Ptr32;
use si_x87::{fmov, x, F80};

use crate::basis::{basis_of, matrix_bezier, matrix_precision, update_matrix};
use crate::edit::{add_point, copy, delete_selection, key_allocate, touch};
use crate::eval::{value, ONE_THIRD, TWO_THIRDS};
use crate::{held, mv, Curve, Host, Spline, SplineKey, Vector, BEZIER, SEGMENT_LINEAR};

/// A newly allocated spline as `SI_splineAllocate` leaves it, named from the counter.
fn allocate(counter: &mut i32) -> Curve {
    let (rec, name) = Spline::allocate(counter);
    let mut c = Curve { rec, name: Some(name), keys: Vec::new(), lengths: Vec::new() };
    c.rec.name = held();
    c
}

fn set_keys(c: &mut Curve, count: i32) {
    c.keys = key_allocate(count);
    c.rec.keys = if c.keys.is_empty() { Ptr32::NULL } else { held() };
}

/// Sets a key's position and both handles to `p` and clears its flag.
fn place(k: &mut SplineKey, p: [f32; 3]) {
    let v = Vector { x: p[0], y: p[1], z: p[2] };
    (k.pos, k.next, k.prev) = (v, v, v);
    k.flag = 0;
}

/// The geometry of segment `seg` as the stepping routines take it; `prev` is the key the
/// last Bézier segment ended on (the routine walks from key to key rather than indexing).
fn geometry(c: &Curve, seg: i32, from: &mut usize) -> Matrix {
    let nb = c.rec.nb_keys as u16 as i32;
    let row = |v: Vector| [fmov(v.x), fmov(v.y), fmov(v.z), 1.0];
    let mut g: Matrix = [[0.0, 0.0, 0.0, 1.0]; 4];
    if c.rec.kind == BEZIER {
        let to = if seg == nb - 1 { 0 } else { seg as usize + 1 };
        let (k0, k1) = (c.key(*from), c.key(to));
        g[0] = row(k0.pos);
        g[3] = row(k1.pos);
        if k1.segment == SEGMENT_LINEAR {
            let mix = |a: f32, b: f32, wa: f64, wb: f64| (x(wa) * a + x(wb) * b).to_f32();
            for j in 0..3 {
                let (a, b) = ([k0.pos.x, k0.pos.y, k0.pos.z][j], [k1.pos.x, k1.pos.y, k1.pos.z][j]);
                g[1][j] = mix(a, b, TWO_THIRDS, ONE_THIRD);
                g[2][j] = mix(a, b, ONE_THIRD, TWO_THIRDS);
            }
        } else {
            g[1] = row(k0.next);
            g[2] = row(k1.prev);
        }
        *from = to;
    } else {
        for (k, r) in g.iter_mut().enumerate() {
            let p = c.key(((seg + k as i32) % nb.max(1)) as usize).pos;
            *r = [p.x, p.y, p.z, 1.0];
        }
    }
    g
}

/// A linear spline through `steps` points a segment
/// [soft 0x0848c3ac splineConvertToLinearRegular]: the step subdivision. `None` where the original writes
/// the first point through the null pointer its allocation of no keys returned (an open
/// source with segments and a negative step count). A source without segments, or with a
/// negative count of them (an open B-spline or cardinal spline of one or two keys), gives a
/// spline without points; when the room asked for is not positive it has no key block.
///
/// The segments are `keys − 1 + closed`, less 2 for an open B-spline or cardinal spline.
/// The new spline is allocated ([`Spline::allocate`]: type 0, named from the counter),
/// takes the source's closed flag and a step of 1, and gets `steps × segments + 1` keys.
/// Each segment is walked by forward differences exactly as the length table is
/// ([`crate::length::update_length`]): F = E(1/steps) × basis × geometry, then per step row
/// 3 += row 2, row 2 += row 1, row 1 += row 0 and the new point is row 3. The start point of
/// the first segment is emitted only for an open curve, so a closed curve yields
/// `steps × segments` points, the last of them the (accumulated) return to the start. Every
/// point's handles equal its position. Class: model.
pub fn convert_to_linear_regular(c: &Curve, steps: i32, counter: &mut i32) -> Option<Curve> {
    let nb = c.rec.nb_keys as u16 as i32;
    let mut segs = nb - 1 + c.rec.closed as i32;
    if c.rec.closed == 0 && (c.rec.kind == 2 || c.rec.kind == 3) {
        segs -= 2;
    }
    let mut out = allocate(counter);
    out.rec.closed = c.rec.closed;
    let room = steps.wrapping_mul(segs).wrapping_add(1);
    set_keys(&mut out, room);
    if out.keys.is_empty() && c.rec.closed == 0 && segs > 0 {
        return None;
    }
    out.rec.step = 1;
    let d = matrix_multiply(&matrix_precision(steps), &basis_of(c.rec.kind, c.rec.tension));
    let mut from = 0usize;
    for seg in 0..segs.max(0) {
        let g = geometry(c, seg, &mut from);
        let mut f = matrix_multiply(&d, &g);
        if out.rec.nb_keys == 0 && out.rec.closed == 0 {
            if let Some(k) = out.keys.get_mut(0) {
                place(k, [f[3][0], f[3][1], f[3][2]]);
            }
            out.rec.nb_keys = out.rec.nb_keys.wrapping_add(1);
        }
        for _ in 0..steps.max(0) {
            for r in (1..4).rev() {
                for j in 0..4 {
                    f[r][j] = (x(f[r][j]) + f[r - 1][j]).to_f32();
                }
            }
            // with a closed source the block has one record to spare, never one too few
            if let Some(k) = out.keys.get_mut(out.rec.nb_keys as u16 as usize) {
                place(k, [f[3][0], f[3][1], f[3][2]]);
            }
            out.rec.nb_keys = out.rec.nb_keys.wrapping_add(1);
        }
    }
    update_matrix(&mut out);
    Some(out)
}

/// A linear spline with as few points as a tolerance allows
/// [soft 0x0848e928 splineConvertToLinearFlatness].
///
/// The curve is first stepped at 100 points a segment ([`convert_to_linear_regular`]). Then,
/// from a kept point `base` (at first point 0) and for i = 1, 2, …: the unit direction from
/// the base to point i + 1 is formed (`SI_vectorNormalize`), and every point from base + 1
/// to i is tested against the line through the base in that direction: the squared length of
/// (point − base) × direction, in the register, against `float(tolerance²)`. As soon as one
/// lies **beyond** it, point i is kept: the points strictly between the base and i are
/// selected, and i becomes the base. At the end the points after the last base but the last
/// are selected, the last one too when the curve is closed, and the selected points are
/// deleted ([`delete_selection`]). Class: model.
pub fn convert_to_linear_flatness(c: &Curve, tolerance: f32, counter: &mut i32, host: &mut dyn Host) -> Option<Curve> {
    let tol2 = (x(tolerance) * tolerance).to_f32();
    let mut out = convert_to_linear_regular(c, 100, counter)?;
    if out.rec.keys.is_null() {
        // the original reads the first key through a null pointer
        return None;
    }
    let pos = |o: &Curve, i: usize| o.key(i).pos;
    let mut base = 0usize;
    let mut anchor = { let p = pos(&out, 0); Vector { x: fmov(p.x), y: fmov(p.y), z: fmov(p.z) } };
    let mut i = 1i32;
    while i < out.rec.nb_keys as u16 as i32 - 1 {
        let to = pos(&out, i as usize + 1);
        let mut dir = si_math::types::Vector::new((x(to.x) - anchor.x).to_f32(), (x(to.y) - anchor.y).to_f32(), (x(to.z) - anchor.z).to_f32());
        vector_normalize(&mut dir);
        let first = base + 1;
        let mut beyond = false;
        for j in first..=(i as usize) {
            let p = pos(&out, j);
            let (vx, vy, vz) = (x(p.x) - anchor.x, x(p.y) - anchor.y, x(p.z) - anchor.z);
            let cx = vy * dir.z - vz * dir.y;
            let cy = vz * dir.x - vx * dir.z;
            let cz = vx * dir.y - vy * dir.x;
            if (cz * cz + (cy * cy + cx * cx)).gt(x(tol2)) {
                beyond = true;
                break;
            }
        }
        if beyond {
            for j in first..(i as usize) {
                out.keys[j].flag |= 1;
            }
            base = i as usize;
            let p = pos(&out, base);
            anchor = Vector { x: fmov(p.x), y: fmov(p.y), z: fmov(p.z) };
        }
        i += 1;
    }
    let last = out.rec.nb_keys as u16 as i32 - 1;
    let mut j = base as i32 + 1;
    while j < last {
        out.keys[j as usize].flag |= 1;
        j += 1;
    }
    if out.rec.closed != 0 && last >= 0 {
        // without keys the original sets a bit in the four bytes before the block
        out.keys[last as usize].flag |= 1;
    }
    delete_selection(&mut out, host);
    Some(out)
}

/// A linear spline that follows a spline [soft 0x0848beec SI_splineConvertToLinear].
/// `None` for a spline without keys. A linear spline is copied. Otherwise with `regular`
/// set: `steps` points a segment (a negative count means the spline's own step, read as an
/// unsigned 16-bit number; 0 means 1). With `regular` clear: by tolerance, its absolute
/// value, replaced by the float nearest 0.0001 when below the double 0.0001.
pub fn convert_to_linear(c: &Curve, regular: i16, steps: i32, tolerance: f32, counter: &mut i32, host: &mut dyn Host) -> Option<Curve> {
    if c.rec.nb_keys == 0 {
        return None;
    }
    if c.rec.kind == 0 {
        return Some(copy(c, counter));
    }
    if regular != 0 {
        let steps = if steps < 0 { c.rec.step as u16 as i32 } else { steps };
        return convert_to_linear_regular(c, if steps == 0 { 1 } else { steps }, counter);
    }
    let mut t = x(tolerance);
    if F80::ZERO.gt(t) {
        t = -t;
    }
    if t.lt(x(1e-4f64)) {
        t = x(f32::from_bits(0x38d1_b717));
    }
    convert_to_linear_flatness(c, t.to_f32(), counter, host)
}

/// `2 pos − other`, rounded once: the handle that mirrors `other` through the position.
fn mirror(pos: Vector, other: Vector) -> Vector {
    Vector { x: ((x(pos.x) + pos.x) - other.x).to_f32(), y: ((x(pos.y) + pos.y) - other.y).to_f32(), z: ((x(pos.z) + pos.z) - other.z).to_f32() }
}

/// What the three routines that build a Bézier spline do to an open result: the first key's
/// `prev` handle and the last key's `next` handle mirror the handle on the curve's side,
/// and both keys get constraint 2. Without keys the original writes through a null pointer.
fn open_ends(c: &mut Curve) {
    let n = c.nb();
    if n == 0 || c.keys.len() < n {
        return;
    }
    c.keys[0].prev = mirror(c.keys[0].pos, c.keys[0].next);
    c.keys[0].constraint = 2;
    c.keys[n - 1].next = mirror(c.keys[n - 1].pos, c.keys[n - 1].prev);
    c.keys[n - 1].constraint = 2;
}

/// A Bézier spline with the same shape [soft 0x0848ee64 SI_splineConvertToBezier]. `None`
/// for a spline without keys (and where the original breaks down: an open B-spline or
/// cardinal spline of two keys; one of a single key counts `1 − 2` keys in 16 bits and
/// really builds 65,535 of them). A Bézier spline is copied.
///
/// **From a linear spline**: one key a key. Key i keeps its position, its `next` handle is
/// `p + 0.3333333 × (q − p)` and the following key's `prev` handle `p + 0.6666667 × (q − p)`
/// (q the following key, the first after the last; the constants are doubles, the difference
/// and the product stay in the register), and the following key gets segment type 2 and
/// constraint 1. So every segment is marked straight.
///
/// **From a B-spline or cardinal spline**: one key a segment start (`keys − 2` for an open
/// source, `keys` for a closed one). With C = Bézier basis⁻¹ × source basis (the inverse by
/// `SI_matrixInverse`, both products rounded to float), the four source positions k₀…k₃ of a
/// segment give the new key's position `C[0]·k`, its `next` handle `C[1]·k` and the
/// following new key's `prev` handle `C[2]·k`, each `float(((c₀k₀ + c₁k₁) + c₂k₂) + c₃k₃)`.
/// The new key gets constraint 2 and segment type 1.
///
/// The result is type 1 with a default step, its matrix updated; an open result's end
/// handles are mirrored (the routine's last step). Class: model.
pub fn convert_to_bezier(c: &Curve, counter: &mut i32) -> Option<Curve> {
    if c.rec.nb_keys == 0 {
        return None;
    }
    if c.rec.kind == 1 {
        return Some(copy(c, counter));
    }
    let nb = c.nb();
    let mut out = allocate(counter);
    out.rec.closed = c.rec.closed;
    if c.rec.kind == 0 {
        out.rec.nb_keys = c.rec.nb_keys;
        set_keys(&mut out, nb as i32);
        for i in 0..nb {
            let to = if i == nb - 1 { 0 } else { i + 1 };
            let (p, q) = (c.key(i).pos, c.key(to).pos);
            let at = |p: f32, q: f32, w: f64| (x(w) * (x(q) - p) + p).to_f32();
            out.keys[i].pos = Vector { x: fmov(p.x), y: fmov(p.y), z: fmov(p.z) };
            out.keys[i].next = Vector { x: at(p.x, q.x, ONE_THIRD), y: at(p.y, q.y, ONE_THIRD), z: at(p.z, q.z, ONE_THIRD) };
            out.keys[to].prev = Vector { x: at(p.x, q.x, TWO_THIRDS), y: at(p.y, q.y, TWO_THIRDS), z: at(p.z, q.z, TWO_THIRDS) };
            out.keys[to].segment = 2;
            out.keys[to].constraint = 1;
        }
    } else {
        out.rec.nb_keys = if c.rec.closed == 0 { c.rec.nb_keys.wrapping_sub(2) } else { c.rec.nb_keys };
        let count = out.nb();
        if count == 0 {
            // an open source of two keys: the original goes on to write the end handles
            // through the null pointer its allocation of no keys returned
            return None;
        }
        set_keys(&mut out, count as i32);
        let inv = matrix_inverse(&matrix_bezier()).unwrap_or([[0.0; 4]; 4]);
        let m = matrix_multiply(&inv, &basis_of(c.rec.kind, c.rec.tension));
        let after = |i: usize| if i == nb - 1 { 0 } else { i + 1 };
        let (mut i0, mut i1) = (0usize, (nb != 1) as usize);
        let mut i2 = after(i1);
        for j in 0..count {
            let to = if j == count - 1 { 0 } else { j + 1 };
            let i3 = after(i2);
            let (k0, k1, k2, k3) = (c.key(i0).pos, c.key(i1).pos, c.key(i2).pos, c.key(i3).pos);
            let dot = |r: usize| {
                let one = |a: f32, b: f32, cc: f32, d: f32| (((x(m[r][0]) * a + x(m[r][1]) * b) + x(m[r][2]) * cc) + x(m[r][3]) * d).to_f32();
                Vector { x: one(k0.x, k1.x, k2.x, k3.x), y: one(k0.y, k1.y, k2.y, k3.y), z: one(k0.z, k1.z, k2.z, k3.z) }
            };
            out.keys[j].pos = dot(0);
            out.keys[j].next = dot(1);
            out.keys[to].prev = dot(2);
            out.keys[j].constraint = 2;
            out.keys[j].segment = 1;
            (i0, i1, i2) = (i1, i2, i3);
        }
    }
    out.rec.kind = 1;
    update_matrix(&mut out);
    if c.rec.closed == 0 {
        open_ends(&mut out);
    }
    Some(out)
}

/// The doubles [`bezier_expand`] places the control points of a straight segment at (and its
/// image form): 0.33333 and 0.66667.
pub(crate) const NEAR: f64 = f64::from_bits(0x3fd5_5547_5a31_a4be);
pub(crate) const FAR: f64 = f64::from_bits(0x3fe5_555c_52e7_2da1);

/// The control points of a Bézier spline as plain keys, three to a segment
/// [soft 0x0848ff8c SI_bezierExpand]: position, `next` handle, the following key's `prev` handle,
/// position, … (`3 × keys − 2` points, or `3 × keys` for a closed spline, which ends on the
/// two control points of the closing segment). Only the positions of the new keys are set.
/// A segment whose end key has segment type 2 gets control points at **0.33333** and
/// **0.66667** of the chord (doubles of five digits, not the seven-digit thirds used
/// elsewhere). Another type of spline is copied. The result is type 1 with the source's
/// closed flag and step. `None` where the original runs off the source's keys (an open
/// spline without keys). Class: model.
pub fn bezier_expand(c: &Curve, counter: &mut i32) -> Option<Curve> {
    if c.rec.kind != 1 {
        return Some(copy(c, counter));
    }
    let nb = c.nb();
    if nb == 0 && c.rec.closed == 0 {
        return None;
    }
    let mut out = allocate(counter);
    out.rec.closed = c.rec.closed;
    out.rec.step = c.rec.step;
    out.rec.kind = 1;
    update_matrix(&mut out);
    out.rec.nb_keys = if c.rec.closed == 0 { c.rec.nb_keys.wrapping_mul(3).wrapping_sub(2) } else { c.rec.nb_keys.wrapping_mul(3) };
    let count = out.nb();
    set_keys(&mut out, count as i32);
    let mut from = 0usize;
    let mut i = 0usize;
    while i < count {
        let p = c.key(from);
        out.keys[i].pos = p.pos;
        if count <= i + 1 {
            break;
        }
        let to = if from == nb - 1 { 0 } else { from + 1 };
        let q = c.key(to);
        if q.segment == SEGMENT_LINEAR {
            let at = |p: f32, q: f32, w: f64| (x(p) + x(w) * (x(q) - p)).to_f32();
            out.keys[i + 1].pos = Vector { x: at(p.pos.x, q.pos.x, NEAR), y: at(p.pos.y, q.pos.y, NEAR), z: at(p.pos.z, q.pos.z, NEAR) };
            out.keys[i + 2].pos = Vector { x: at(p.pos.x, q.pos.x, FAR), y: at(p.pos.y, q.pos.y, FAR), z: at(p.pos.z, q.pos.z, FAR) };
        } else {
            out.keys[i + 1].pos = p.next;
            out.keys[i + 2].pos = q.prev;
        }
        i += 3;
        from = to;
    }
    Some(out)
}

/// The reverse of [`bezier_expand`] [soft 0x08490168 SI_bezierShrink]: keys with handles
/// from control points three to a segment. The new spline has `(points − 1) / 3 + 1` keys
/// (`points / 3` when closed); point 3i is key i's position, point 3i + 1 its `next` handle
/// and point 3i + 2 the following key's `prev` handle (the first key's, after the last). An
/// open result's end handles are mirrored and its end keys get constraint 2. Another type of
/// spline is copied. This is what the model file reader calls for the old key layout
/// (positions only: finding 0004). `None` where the original writes through a null pointer
/// (a closed source of one or two points). Class: model.
pub fn bezier_shrink(c: &Curve, counter: &mut i32) -> Option<Curve> {
    if c.rec.kind != 1 {
        return Some(copy(c, counter));
    }
    let nb = c.nb();
    let mut out = allocate(counter);
    out.rec.closed = c.rec.closed;
    out.rec.step = c.rec.step;
    out.rec.kind = 1;
    update_matrix(&mut out);
    out.rec.nb_keys = if c.rec.closed == 0 { (((nb as i32 - 1) / 3) as i16).wrapping_add(1) } else { (c.rec.nb_keys as u16 / 3) as i16 };
    let count = out.nb();
    set_keys(&mut out, count as i32);
    if count == 0 && nb != 0 {
        return None;
    }
    let mut to = 0usize;
    let mut i = 0usize;
    while i < nb {
        out.keys[to].pos = c.key(i).pos;
        if nb <= i + 1 {
            break;
        }
        out.keys[to].next = c.key(i + 1).pos;
        to = if to == count - 1 { 0 } else { to + 1 };
        if nb <= i + 2 {
            break;
        }
        out.keys[to].prev = c.key(i + 2).pos;
        i += 3;
    }
    if out.rec.closed == 0 {
        open_ends(&mut out);
    }
    Some(out)
}

/// Removes the superfluous points of a linear spline of more than two keys
/// [soft 0x0848eb28 SI_splineCleanup]; other splines are left alone.
///
/// All selection bits are cleared first. Then each interior point is tested with
/// `SI_pointsColinear` against the last point kept before it and the point after it, and
/// selected when the three are on one line (squared cross product below 1e-8, absolute). On
/// a closed spline the first point is tested between the last unselected point and the first
/// unselected point after it, and then the last point between the last unselected point
/// before it and the first unselected point of the whole spline. The selected points are
/// deleted ([`delete_selection`]).
pub fn cleanup(c: &mut Curve, host: &mut dyn Host) {
    let nb = c.nb();
    if c.rec.kind != 0 || nb <= 2 || c.keys.len() < nb {
        return;
    }
    for k in &mut c.keys[..nb] {
        k.flag &= !1;
    }
    let colinear = |b: Vector, a: Vector, cc: Vector| points_colinear(&mv(b), &mv(a), &mv(cc));
    let mut a = c.keys[0].pos;
    let mut b = c.keys[1].pos;
    for i in 1..nb - 1 {
        let next = c.keys[i + 1].pos;
        if colinear(b, a, next) {
            c.keys[i].flag |= 1;
        } else {
            a = b;
        }
        b = next;
    }
    if c.rec.closed != 0 {
        let selected = |c: &Curve, i: usize| c.keys[i].flag & 1 != 0;
        // the first point, between the last and the next unselected ones
        if let Some(before) = (0..nb).rev().find(|&i| !selected(c, i)) {
            if let Some(after) = (1..nb).find(|&i| !selected(c, i)) {
                if colinear(c.keys[0].pos, c.keys[before].pos, c.keys[after].pos) {
                    c.keys[0].flag |= 1;
                }
            }
        }
        // the last point, between the unselected one before it and the first unselected one
        if let Some(before) = (0..nb - 1).rev().find(|&i| !selected(c, i)) {
            if let Some(after) = (0..nb).find(|&i| !selected(c, i)) {
                if colinear(c.keys[nb - 1].pos, c.keys[before].pos, c.keys[after].pos) {
                    c.keys[nb - 1].flag |= 1;
                }
            }
        }
    }
    delete_selection(c, host);
}

/// Splits segment `seg` at the local parameter `t` by inserting a point
/// [soft 0x08492294 splineSubdivision]; false when nothing was done: a B-spline or cardinal spline, a type
/// above 3, or `t` below 0.0001 or above 0.9999 (doubles).
///
/// **Linear**: the point at `float(seg + t)` ([`value`]) is added after key `seg`.
///
/// **Bézier**: de Casteljau's construction on the segment's four control points P0…P3 with
/// w = float(1 − t). In the original's order of operations (r = rounded to float, everything
/// else in the register):
///
/// - A = t·P1 + w·P0; D = t·P3 + w·P2;
/// - B = t·P2 + w·P1, where for y and z the product w·P1 is rounded first, and for y and z B
///   itself is rounded before it is used again;
/// - C = t·B + r(w·A); E = t·D + w·B (for x: `t·D + r(w·B)`);
/// - the new point F = t·E + w·r(C).
///
/// The point F is added after key `seg` ([`add_point`]); then key `seg`'s `next` handle
/// becomes A, the new key's handles C and E, the following key's `prev` handle D, and all
/// three keys get constraint 1. The spline is touched again.
///
/// **When the key block has to grow for the new point** (the count was a multiple of 16)
/// the original still writes A and D through pointers into the block it has just handed
/// back to the allocator; whether those writes land in the spline depends on the allocator.
/// The port writes them to the keys they were meant for. Class: model.
pub fn subdivision(c: &mut Curve, seg: i32, t: f32, host: &mut dyn Host) -> bool {
    let kind = c.rec.kind;
    if (kind.wrapping_sub(2) as u32) < 2 || x(t).lt(x(1e-4f64)) || x(t).gt(x(0.9999f64)) {
        return false;
    }
    let nb = c.rec.nb_keys as u16 as i32;
    let (i0, i1) = if seg == nb - 1 { (seg, 0) } else { (seg, seg + 1) };
    let w = (F80::ONE - t).to_f32();
    if kind == 1 {
        let (k0, k1) = (c.key(i0 as usize), c.key(i1 as usize));
        let (p0, p1, p2, p3) = (k0.pos, k0.next, k1.prev, k1.pos);
        let lerp = |a: f32, b: f32| x(t) * b + x(w) * a; // t·b + w·a, unrounded
        let a = [lerp(p0.x, p1.x), lerp(p0.y, p1.y), lerp(p0.z, p1.z)];
        let d = [lerp(p2.x, p3.x), lerp(p2.y, p3.y), lerp(p2.z, p3.z)];
        let bx = lerp(p1.x, p2.x);
        let by = (x(t) * p2.y + (x(w) * p1.y).to_f32()).to_f32();
        let bz = (x(t) * p2.z + (x(w) * p1.z).to_f32()).to_f32();
        let cx = (x(t) * bx + (a[0] * x(w)).to_f32()).to_f32();
        let cy = (x(t) * by + (a[1] * x(w)).to_f32()).to_f32();
        let cz = (x(t) * bz + (a[2] * x(w)).to_f32()).to_f32();
        let ex = d[0] * x(t) + (bx * x(w)).to_f32();
        let ey = x(t) * d[1] + x(w) * by;
        let ez = x(t) * d[2] + x(w) * bz;
        let f = Vector { x: (ex * x(t) + x(w) * cx).to_f32(), y: (ey * x(t) + x(w) * cy).to_f32(), z: (x(w) * cz + ez * x(t)).to_f32() };
        let v = |p: [F80; 3]| Vector { x: p[0].to_f32(), y: p[1].to_f32(), z: p[2].to_f32() };
        let new = add_point(c, seg + 1, &f, host);
        // the following key has moved up one record unless it is the first key
        let i1 = if seg != c.rec.nb_keys as u16 as i32 - 2 { i1 + 1 } else { i1 };
        if let Some(k) = c.keys.get_mut(i0 as usize) {
            k.next = v(a);
            k.constraint = 1;
        }
        if let Some(k) = c.keys.get_mut(new) {
            k.prev = Vector { x: cx, y: cy, z: cz };
            k.next = v([ex, ey, ez]);
            k.constraint = 1;
        }
        if let Some(k) = c.keys.get_mut(i1 as usize) {
            k.prev = v(d);
            k.constraint = 1;
        }
    } else if kind == 0 {
        let p = value(c, (F80::from_i32(seg) + t).to_f32()).0;
        add_point(c, seg + 1, &p, host);
    } else {
        return false;
    }
    touch(c, host);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoHost, BSPLINE, CARDINAL, LINEAR};

    fn pts(n: usize) -> Vec<[f32; 3]> {
        (0..n).map(|i| [i as f32, ((i * i) % 5) as f32, 0.25 * i as f32]).collect()
    }

    #[test]
    fn regular_steps_follow_the_curve() {
        let mut n = 0;
        let mut c = Curve::from_points(CARDINAL, false, &pts(6));
        c.rec.tension = 0.5;
        let l = convert_to_linear_regular(&c, 4, &mut n).unwrap();
        assert_eq!((l.nb(), l.rec.kind, l.rec.step, l.name.as_deref()), (13, 0, 1, Some(&b"spl1"[..])));
        assert_eq!(l.keys[0].pos, c.keys[1].pos);
        // a step of forward differences agrees with direct evaluation to rounding
        let direct = value(&c, 1.25).0;
        assert!((l.keys[5].pos.x - direct.x).abs() < 1e-5 && (l.keys[5].pos.y - direct.y).abs() < 1e-5);
        let closed = Curve::from_points(BSPLINE, true, &pts(5));
        assert_eq!(convert_to_linear_regular(&closed, 3, &mut n).unwrap().nb(), 15);
        assert_eq!(convert_to_linear_regular(&Curve::from_points(BSPLINE, false, &pts(1)), 100, &mut n).unwrap().keys.len(), 0);
        assert!(convert_to_linear_flatness(&Curve::from_points(BSPLINE, false, &pts(1)), 0.1, &mut n, &mut NoHost).is_none());
        assert!(convert_to_linear(&Curve::new(), 1, 3, 0.0, &mut n, &mut NoHost).is_none());
    }

    #[test]
    fn flatness_keeps_few_points_on_a_gentle_curve() {
        let mut n = 0;
        let mut c = Curve::from_points(CARDINAL, false, &[[0.0; 3], [0.0; 3], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 0.0, 0.0]]);
        c.rec.tension = 0.5;
        let l = convert_to_linear(&c, 0, 0, 0.01, &mut n, &mut NoHost).unwrap();
        assert_eq!(l.nb(), 2); // a straight line
        let mut bent = Curve::from_points(CARDINAL, false, &pts(6));
        bent.rec.tension = 0.5;
        let coarse = convert_to_linear(&bent, 0, 0, 0.5, &mut n, &mut NoHost).unwrap().nb();
        let fine = convert_to_linear(&bent, 0, 0, -0.001, &mut n, &mut NoHost).unwrap().nb();
        assert!(coarse >= 2 && coarse < fine && fine < 301, "{coarse} {fine}");
    }

    #[test]
    fn bezier_from_the_other_types_has_the_same_points() {
        let mut n = 0;
        for (kind, closed) in [(LINEAR, false), (LINEAR, true), (BSPLINE, false), (BSPLINE, true), (CARDINAL, false), (CARDINAL, true)] {
            let mut c = Curve::from_points(kind, closed, &pts(6));
            c.rec.tension = 0.5;
            let b = convert_to_bezier(&c, &mut n).unwrap();
            assert_eq!(b.rec.kind, 1);
            let segs = crate::eval::nb_segment(&c);
            assert_eq!(crate::eval::nb_segment(&b), segs, "{kind} {closed}");
            for s in 0..segs {
                for u in [0.0f32, 0.3, 0.75] {
                    let (p, q) = (value(&c, s as f32 + u).0, value(&b, s as f32 + u).0);
                    assert!((p.x - q.x).abs() < 1e-4 && (p.y - q.y).abs() < 1e-4 && (p.z - q.z).abs() < 1e-4, "{kind} {closed} {s} {u}: {p:?} {q:?}");
                }
            }
        }
    }

    #[test]
    fn expand_and_shrink_are_inverse() {
        let mut n = 0;
        for closed in [false, true] {
            let mut c = Curve::from_points(CARDINAL, closed, &pts(7));
            c.rec.tension = 0.5;
            let b = convert_to_bezier(&c, &mut n).unwrap();
            let e = bezier_expand(&b, &mut n).unwrap();
            assert_eq!(e.nb(), if closed { 3 * b.nb() } else { 3 * b.nb() - 2 });
            let s = bezier_shrink(&e, &mut n).unwrap();
            assert_eq!(s.nb(), b.nb());
            for (x, y) in s.used().iter().zip(b.used()) {
                assert_eq!((x.pos, x.next, x.prev), (y.pos, y.next, y.prev), "{closed}");
            }
        }
        let mut l = Curve::from_points(LINEAR, true, &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0], [0.0, 1.0, 0.0]]);
        cleanup(&mut l, &mut NoHost);
        assert_eq!(l.used().iter().map(|k| (k.pos.x, k.pos.y)).collect::<Vec<_>>(), [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    }

    #[test]
    fn a_split_segment_keeps_its_shape() {
        let mut n = 0;
        let mut c = Curve::from_points(CARDINAL, false, &pts(6));
        c.rec.tension = 0.5;
        let mut b = convert_to_bezier(&c, &mut n).unwrap();
        let before: Vec<Vector> = (0..=8).map(|i| value(&b, 1.0 + i as f32 / 8.0).0).collect();
        assert!(subdivision(&mut b, 1, 0.5, &mut NoHost));
        assert_eq!(b.nb(), 5);
        for (i, p) in before.iter().enumerate() {
            let q = value(&b, 1.0 + i as f32 / 4.0).0;
            assert!((p.x - q.x).abs() < 1e-4 && (p.y - q.y).abs() < 1e-4, "{i}: {p:?} {q:?}");
        }
        assert!(!subdivision(&mut b, 1, 0.00005, &mut NoHost));
        assert!(!subdivision(&mut c, 1, 0.5, &mut NoHost));
        let mut l = Curve::from_points(LINEAR, false, &pts(3));
        assert!(subdivision(&mut l, 0, 0.25, &mut NoHost));
        assert_eq!((l.nb(), l.keys[1].pos.x), (4, 0.25));
    }
}
