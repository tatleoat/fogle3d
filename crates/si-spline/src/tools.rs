// SPDX-License-Identifier: MIT
//! Spline routines the modelling tools call: the cubic solver, random displacement, moving
//! a Bézier control point under its constraint, and merging two splines (finding 0020,
//! section 14).

use si_math::algebra::points_colinear;
use si_types::Ptr32;
use si_x87::{fmov, trig, x, F80};

use crate::basis::update_matrix;
use crate::edit::{key_allocate, touch};
use crate::patch_edit::Drand48;
use crate::{held, mv, Curve, Host, Spline, SplineKey, Vector, BEZIER};

/// The constraint of a Bézier key (+0x28), as `SI_bezierMovePoint` acts on it.
/// 0 and 1: the two handles are independent (a corner).
pub const CONSTRAINT_CORNER: i16 = 1;
/// 2 and 3: a moved handle is mirrored through the point (equal lengths).
pub const CONSTRAINT_MIRROR: i16 = 2;
/// 4: the other handle is kept on the line through the point, at its own length.
pub const CONSTRAINT_ALIGNED: i16 = 4;

/// The real roots of `c[0] + c[1] x + c[2] x² + c[3] x³ = target`
/// [soft 0x08491f70 SI_solveCubic].
///
/// **`c[0]` is replaced by `float(c[0] − target)`** (the difference itself goes on in the
/// register). Every "is zero" test is `|v| < 1e-6` against the double constant.
///
/// - `|c[3]|` below 1e-6, `|c[2]|` below too: one root `−c0 / c[1]`, or none when `|c[1]|`
///   is below as well;
/// - `|c[3]|` below, quadratic: with `h = c[1] / (c[2] + c[2])` and `d = h² − c0 / c[2]`:
///   `|d|` below 1e-6: one root `−h`; d below −1e-6: none; d above 1e-6: `√d − h` and
///   `−√d − h` (√d rounded to float first);
/// - cubic: `a = float(c[2] / c[3])`, `p = float((a² × −⅓ + c[1]/c[3]) × ⅓)`,
///   `q = float((c0/c[3] + (a² × (a × 2/27) − (c[1]/c[3]) × (a × ⅓))) × ½)`,
///   `D = q² + p³`. `|D|` below 1e-6: the root 0 when `|q|` is below too, else with
///   `u = float(cbrt(−q))` the roots `u + u` and `−u`. D negative: three roots
///   `m cos(φ)`, `−m cos(φ + π/3)`, `−m cos(φ − π/3)` with
///   `φ = float(acos(−q / √(−p³)) × ⅓)` and `m = float(√(−p) + √(−p))`. D positive: one root
///   `float(−cbrt(√D + q)) + float(cbrt(√D − q))` (√D a float). Every root then has
///   `float(⅓ × a)` subtracted.
///
/// The thirds, 2/27, a half and π/3 are doubles; `cbrt`, `acos` and `cos` are the C
/// library's. Returns the roots and their number; `None` where the original returns
/// whatever its count register held: a quadratic whose `d` is exactly the constant 1e-6, or
/// not a number.
pub fn solve_cubic(c: &mut [f32; 4], target: f32) -> Option<([f32; 3], i16)> {
    let eps = x(1.0e-6f64);
    let c3 = c[3];
    let c0 = x(c[0]) - target;
    c[0] = c0.to_f32();
    let mut r = [0.0f32; 3];
    if x(c3).abs().lt(eps) {
        if x(c[2]).abs().lt(eps) {
            if x(c[1]).abs().lt(eps) {
                return Some((r, 0));
            }
            r[0] = ((-c0) / c[1]).to_f32();
            return Some((r, 1));
        }
        let h = x(c[1]) / (x(c[2]) + c[2]);
        let d = (h * h) - (c0 / c[2]);
        if d.abs().lt(eps) {
            r[0] = (-h).to_f32();
            return Some((r, 1));
        }
        if eps.gt(d) {
            return Some((r, 0));
        }
        if !d.gt(eps) {
            return None;
        }
        let s = d.sqrt().to_f32();
        r[0] = (x(s) - h).to_f32();
        r[1] = ((-x(s)) - h).to_f32();
        return Some((r, 2));
    }
    let third = x(f64::from_bits(0x3fd5_5555_5555_5555));
    let a = (x(c[2]) / c3).to_f32();
    let a2 = x(a) * a;
    let p = (((a2 * x(f64::from_bits(0xbfd5_5555_5555_5555))) + (x(c[1]) / c3)) * third).to_f32();
    let q = (((c0 / c3) + ((a2 * (x(a) * x(f64::from_bits(0x3fb2_f684_bda1_2f68)))) - ((x(c[1]) / c3) * (x(a) * third)))) * x(0.5f64)).to_f32();
    let p3 = (x(p) * p) * p;
    let disc = (x(q) * q) + p3;
    let n;
    if disc.abs().lt(eps) {
        if x(q).abs().lt(eps) {
            r[0] = 0.0;
            n = 1;
        } else {
            let u = trig::cbrt((-x(q)).to_f64()).to_f32();
            r[1] = (-x(u)).to_f32();
            r[0] = (x(u) + u).to_f32();
            n = 2;
        }
    } else if F80::ZERO.gt(disc) {
        const SIXTY: f64 = f64::from_bits(0x3ff0_c152_382d_7365);
        let arg = ((-x(q)) / (-p3).sqrt()).to_f64();
        let phi = (trig::acos(arg) * third).to_f32();
        let m = ((-x(p)).sqrt() + (-x(p)).sqrt()).to_f32();
        r[0] = (trig::cos(phi as f64) * (m as f64)).to_f32();
        let neg = (-x(m)).to_f64();
        r[1] = (trig::cos((x(phi as f64) + SIXTY).to_f64()) * neg).to_f32();
        r[2] = (trig::cos((x(phi as f64) - SIXTY).to_f64()) * neg).to_f32();
        n = 3;
    } else {
        let s = disc.sqrt().to_f32();
        let u = trig::cbrt((x(s) - q).to_f64()).to_f32();
        let v = (-trig::cbrt((x(s) + q).to_f64())).to_f32();
        r[0] = (x(v) + u).to_f32();
        n = 1;
    }
    let shift = (third * a).to_f32();
    for root in r.iter_mut().take(n as usize) {
        *root = (x(*root) - shift).to_f32();
    }
    Some((r, n))
}

/// Moves keys at random [soft 0x0848bf80 SI_splineRandomize].
///
/// Key by key from the first, `count` times a key: the position's x, y, z each become
/// `float((drand48() − 0.5) × amplitude + coordinate)`; on a Bézier spline the `prev`
/// handle and then the `next` handle get the same treatment (nine calls a turn), on the
/// other types both handles are set to the new position. With `tagged` not 0 only keys
/// whose flag has bit 0. Ends with [`touch`].
pub fn randomize(c: &mut Curve, amplitude: &Vector, count: i16, tagged: i16, rng: &mut Drand48, host: &mut dyn Host) {
    let amp = [amplitude.x, amplitude.y, amplitude.z];
    let jolt = |v: &mut Vector, rng: &mut Drand48| {
        v.x = (((x(rng.next()) - 0.5f64) * amp[0]) + v.x).to_f32();
        v.y = (((x(rng.next()) - 0.5f64) * amp[1]) + v.y).to_f32();
        v.z = (((x(rng.next()) - 0.5f64) * amp[2]) + v.z).to_f32();
    };
    let bezier = c.rec.kind == BEZIER;
    let nb = c.nb();
    for k in c.keys.iter_mut().take(nb) {
        if tagged != 0 && k.flag & 1 == 0 {
            continue;
        }
        for _ in 0..count.max(0) {
            jolt(&mut k.pos, rng);
            if bezier {
                jolt(&mut k.prev, rng);
                jolt(&mut k.next, rng);
            } else {
                k.next = k.pos;
                k.prev = k.next;
            }
        }
    }
    touch(c, host);
}

/// Moves one control point of a Bézier spline [soft 0x084302e0 SI_bezierMovePoint].
///
/// `point` numbers control points three a key (`prev` handle, position, `next` handle:
/// see `SI_bezierGetPoint`). A key outside the spline gives `false` (the original's 0)
/// with nothing done.
///
/// - the position: the key moves as a whole, both handles shifted by
///   `float(new − old)` per coordinate;
/// - a handle: it is set, and then by the key's constraint (+0x28): 2 or 3: the other
///   handle becomes `float((pos + pos) − handle)`; 4: the other handle goes to
///   `pos + d × (its own length / |d|)` with `d = pos − handle`, the scaling skipped when
///   `float(|d|)` is not above 0.0001 (then `pos + float(d)`); anything else: left alone.
///
/// A negative `point` above −3 names key 0 with a remainder that is none of the three:
/// nothing moves, but the spline is touched and the result is 1, as for every valid key.
pub fn bezier_move_point(c: &mut Curve, point: i32, p: &Vector, host: &mut dyn Host) -> bool {
    let k = point / 3;
    if k < 0 || k >= c.nb() as i32 {
        return false;
    }
    let Some(key) = c.keys.get_mut(k as usize) else { return false };
    let aligned = |pos: Vector, moved: Vector, other: Vector| -> Vector {
        let dx = x(pos.x) - moved.x;
        let dy = x(pos.y) - moved.y;
        let dz = x(pos.z) - moved.z;
        let len = (((dy * dy) + (dx * dx)) + (dz * dz)).sqrt().to_f32();
        let (ex, ey, ez);
        if x(len).abs().gt(x(1.0e-4f64)) {
            let (ox, oy, oz) = (x(other.x) - pos.x, x(other.y) - pos.y, x(other.z) - pos.z);
            let scale = (((ox * ox) + (oy * oy)) + (oz * oz)).sqrt() / len;
            ex = (dx * scale).to_f32();
            ez = (dz * scale).to_f32();
            ey = (dy * scale).to_f32();
        } else {
            (ex, ey, ez) = (dx.to_f32(), dy.to_f32(), dz.to_f32());
        }
        Vector { x: (x(pos.x) + ex).to_f32(), y: (x(pos.y) + ey).to_f32(), z: (x(pos.z) + ez).to_f32() }
    };
    let mirror = |pos: Vector, moved: Vector| Vector {
        x: ((x(pos.x) + pos.x) - moved.x).to_f32(),
        y: ((x(pos.y) + pos.y) - moved.y).to_f32(),
        z: ((x(pos.z) + pos.z) - moved.z).to_f32(),
    };
    match point - 3 * k {
        0 => {
            key.prev = *p;
            match key.constraint {
                2 | 3 => key.next = mirror(key.pos, key.prev),
                4 => key.next = aligned(key.pos, key.prev, key.next),
                _ => {}
            }
        }
        1 => {
            let d = [(x(p.x) - key.pos.x).to_f32(), (x(p.y) - key.pos.y).to_f32(), (x(p.z) - key.pos.z).to_f32()];
            key.pos = *p;
            for h in [&mut key.prev, &mut key.next] {
                h.x = (x(h.x) + d[0]).to_f32();
                h.y = (x(h.y) + d[1]).to_f32();
                h.z = (x(h.z) + d[2]).to_f32();
            }
        }
        2 => {
            key.next = *p;
            match key.constraint {
                2 | 3 => key.prev = mirror(key.pos, key.next),
                4 => key.prev = aligned(key.pos, key.next, key.prev),
                _ => {}
            }
        }
        _ => {}
    }
    touch(c, host);
    true
}

fn allocate(counter: &mut i32) -> Curve {
    let (rec, name) = Spline::allocate(counter);
    let mut c = Curve { rec, name: Some(name), keys: Vec::new(), lengths: Vec::new() };
    c.rec.name = held();
    c
}

fn set_keys(c: &mut Curve, count: u16) {
    c.rec.nb_keys = count as i16;
    c.keys = key_allocate(count as i32);
    c.rec.keys = if c.keys.is_empty() { Ptr32::NULL } else { held() };
}

fn used(c: &Curve, i: usize) -> SplineKey {
    c.key(i)
}

/// The constraint a key's handles satisfy, as two routines of the original decide it:
/// 2 when `(pos + pos) − prev − next` is within 0.0001 (double) in every coordinate, else 1,
/// and then 4 when the three points are on one line (`SI_pointsColinear`).
pub(crate) fn natural_constraint(k: &SplineKey) -> i16 {
    let off = |pos: f32, prev: f32, next: f32| (((x(pos) + pos) - prev) - next).abs().gt(x(1.0e-4f64));
    let mut c = 2;
    if off(k.pos.x, k.prev.x, k.next.x) || off(k.pos.y, k.prev.y, k.next.y) || off(k.pos.z, k.prev.z, k.next.z) {
        c = 1;
    }
    if c == 1 && points_colinear(&mv(k.prev), &mv(k.pos), &mv(k.next)) {
        c = 4;
    }
    c
}

/// Joins two open Bézier splines into a new one [soft 0x08430668 SI_bezierMerge]. `None`
/// (the original's null) when either is closed.
///
/// - `mode` 0 or 1: the keys of `a`, then the keys of `b`, whole records; with mode 0 the
///   first key of `b` gets segment type 2 (the joining segment is a straight line). The
///   second index table (`SI_gblIndex2`, 0x08788970), when set, has `a`'s key count added
///   to each of `b`'s entries.
/// - `mode` 2: **welded**: `b` is shifted so that its first key lands on `a`'s last. One
///   key fewer; the result's `offset` is the shift, `float(a.last.pos − b.first.pos)`. The
///   welded key keeps `a`'s position and `prev` handle and takes `b`'s first `next` handle
///   (shifted), and its constraint is decided anew ([`natural_constraint`]). The other keys
///   of `b` are copied shifted, with their constraint and segment type but **not** their
///   flag or the float at +0x24 (those stay as the allocator set them). The first table
///   (`SI_gblIndex`) gets −1 at `a`'s last key; the second, `a`'s count less one added.
/// - any other mode: a Bézier spline without keys.
///
/// The second result is the shift of mode 2 (untouched by the original otherwise).
pub fn bezier_merge(a: &Curve, b: &Curve, mode: i16, counter: &mut i32, host: &mut dyn Host) -> Option<(Curve, Option<Vector>)> {
    if a.rec.closed != 0 || b.rec.closed != 0 {
        return None;
    }
    let mut n = allocate(counter);
    n.rec.kind = BEZIER;
    update_matrix(&mut n);
    let (na, nb) = (a.nb(), b.nb());
    let mut offset = None;
    if (mode as u16) < 2 {
        set_keys(&mut n, (na as u16).wrapping_add(nb as u16));
        for i in 0..na {
            if let Some(k) = n.keys.get_mut(i) {
                *k = used(a, i);
            }
        }
        for i in 0..nb {
            if let Some(k) = n.keys.get_mut(na + i) {
                *k = used(b, i);
                if i == 0 && mode == 0 {
                    k.segment = 2;
                }
            }
            if host.has_index2() {
                host.set_index2(i as i32, host.index2(i as i32).wrapping_add(na as i32));
            }
        }
    } else if mode == 2 {
        set_keys(&mut n, (na as u16).wrapping_add(nb as u16).wrapping_sub(1));
        for i in 0..na {
            if let Some(k) = n.keys.get_mut(i) {
                *k = used(a, i);
            }
        }
        if host.has_index() {
            host.set_index(na as i32 - 1, -1);
        }
        // with no key in `a` the original reads the record before its block
        let last = if na == 0 { <SplineKey as si_types::Zero>::zero() } else { used(a, na - 1) };
        let first = used(b, 0);
        let d = [(x(last.pos.x) - first.pos.x).to_f32(), (x(last.pos.y) - first.pos.y).to_f32(), (x(last.pos.z) - first.pos.z).to_f32()];
        offset = Some(Vector { x: d[0], y: d[1], z: d[2] });
        let shift = |v: Vector| Vector { x: (x(v.x) + d[0]).to_f32(), y: (x(v.y) + d[1]).to_f32(), z: (x(v.z) + d[2]).to_f32() };
        for i in 0..nb {
            let src = used(b, i);
            let at = (na + i).wrapping_sub(1);
            if let Some(k) = n.keys.get_mut(at) {
                if i == 0 {
                    k.next = shift(src.next);
                    k.constraint = natural_constraint(k);
                } else {
                    k.constraint = src.constraint;
                    k.segment = src.segment;
                    k.prev = shift(src.prev);
                    k.pos = shift(src.pos);
                    k.next = shift(src.next);
                }
            }
            if host.has_index2() {
                host.set_index2(i as i32, (na as i32).wrapping_add(host.index2(i as i32)).wrapping_sub(1));
            }
        }
    }
    Some((n, offset))
}

/// Joins two splines of one type into a new one [soft 0x08491c7c SI_splineMerge].
///
/// A spline is allocated with `a`'s phantom word, type, tension and step. Then by `a`'s
/// type:
///
/// - Bézier: the result is [`bezier_merge`]`(a, b, 1)` instead (which may be null); the
///   spline just allocated is abandoned (the name counter advances twice);
/// - linear: the keys of `a`, then the keys of `b`, whole records;
/// - B-spline, cardinal: the keys of `a`, then the keys of `b` **from its fourth**:
///   `a.count + b.count − 3` keys. The last three keys of `a` are then moved towards the
///   first three of `b`: `pos = float(pos + (b.pos − pos) × blend)` (positions only; the
///   handles keep `a`'s). With blend 0.5 the two curves' overlapping control polygons are
///   averaged;
/// - any other type: no keys.
///
/// The second index table is adjusted as the port's body shows (for the B-spline case the
/// original adds `a.count − 3` to entries 0 to `b.count − 4`, sets entries 0, 1, 2 to −1 and
/// then adds `a.count − 3` to the last three entries, in that order).
pub fn merge(a: &Curve, b: &Curve, blend: f32, counter: &mut i32, host: &mut dyn Host) -> Option<Curve> {
    let mut n = allocate(counter);
    n.rec.phantom = a.rec.phantom;
    n.rec.kind = a.rec.kind;
    n.rec.tension = fmov(a.rec.tension);
    n.rec.step = a.rec.step;
    update_matrix(&mut n);
    let (na, nb) = (a.nb(), b.nb());
    match a.rec.kind {
        1 => return bezier_merge(a, b, 1, counter, host).map(|r| r.0),
        0 => {
            set_keys(&mut n, (na as u16).wrapping_add(nb as u16));
            for i in 0..na {
                if let Some(k) = n.keys.get_mut(i) {
                    *k = used(a, i);
                }
            }
            for i in 0..nb {
                if let Some(k) = n.keys.get_mut(na + i) {
                    *k = used(b, i);
                }
                if host.has_index2() {
                    host.set_index2(i as i32, host.index2(i as i32).wrapping_add(na as i32));
                }
            }
        }
        2 | 3 => {
            set_keys(&mut n, (na as u16).wrapping_add(nb as u16).wrapping_sub(3));
            for i in 0..na {
                if let Some(k) = n.keys.get_mut(i) {
                    *k = used(a, i);
                }
            }
            let tail = nb as i32 - 3;
            let add = (na as i32).wrapping_sub(3);
            for i in 0..tail.max(0) {
                if let Some(k) = n.keys.get_mut(na + i as usize) {
                    *k = used(b, i as usize + 3);
                }
                if host.has_index2() {
                    host.set_index2(i, add.wrapping_add(host.index2(i)));
                }
            }
            if host.has_index2() {
                for i in 0..3 {
                    host.set_index2(i, -1);
                }
                for i in tail..nb as i32 {
                    host.set_index2(i, add.wrapping_add(host.index2(i)));
                }
            }
            for i in 0..3usize {
                // with fewer than three keys in `a` the original writes before its block
                let Some(at) = (na + i).checked_sub(3) else { continue };
                let from = used(b, i).pos;
                if let Some(k) = n.keys.get_mut(at) {
                    k.pos.x = (x(k.pos.x) + ((x(from.x) - k.pos.x) * blend)).to_f32();
                    k.pos.y = (x(k.pos.y) + ((x(from.y) - k.pos.y) * blend)).to_f32();
                    k.pos.z = (x(k.pos.z) + ((x(from.z) - k.pos.z) * blend)).to_f32();
                }
            }
        }
        _ => {}
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoHost, BSPLINE, LINEAR};

    #[test]
    fn cubic_roots() {
        // (x − 1)(x − 2)(x − 3) = x³ − 6x² + 11x − 6
        let mut c = [-6.0f32, 11.0, -6.0, 1.0];
        let (r, n) = solve_cubic(&mut c, 0.0).unwrap();
        assert_eq!(n, 3);
        let mut roots = r.to_vec();
        roots.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for (got, want) in roots.iter().zip([1.0f32, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-4, "{roots:?}");
        }
        // x² = 4, as "x² − 1 = 3": the constant term is changed in place
        let mut c = [-1.0f32, 0.0, 1.0, 0.0];
        let (r, n) = solve_cubic(&mut c, 3.0).unwrap();
        assert_eq!((n, r[0], r[1], c[0]), (2, 2.0, -2.0, -4.0));
        let mut c = [1.0f32, 2.0, 0.0, 0.0];
        assert_eq!(solve_cubic(&mut c, 0.0).unwrap(), ([-0.5, 0.0, 0.0], 1));
        let mut c = [1.0f32, 0.0, 0.0, 0.0];
        assert_eq!(solve_cubic(&mut c, 0.0).unwrap().1, 0);
    }

    #[test]
    fn a_moved_handle_follows_its_constraint() {
        let mut c = Curve::from_points(BEZIER, false, &[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0]]);
        c.keys[1].prev = Vector { x: 3.0, y: 0.0, z: 0.0 };
        c.keys[1].next = Vector { x: 6.0, y: 0.0, z: 0.0 };
        c.keys[1].constraint = CONSTRAINT_MIRROR;
        assert!(bezier_move_point(&mut c, 3, &Vector { x: 4.0, y: -1.0, z: 0.0 }, &mut NoHost));
        assert_eq!(c.keys[1].next, Vector { x: 4.0, y: 1.0, z: 0.0 });
        c.keys[1].next = Vector { x: 4.0, y: 3.0, z: 0.0 };
        c.keys[1].constraint = CONSTRAINT_ALIGNED;
        assert!(bezier_move_point(&mut c, 3, &Vector { x: 2.0, y: 0.0, z: 0.0 }, &mut NoHost));
        assert_eq!(c.keys[1].next, Vector { x: 7.0, y: 0.0, z: 0.0 }); // length 3 kept, opposite the handle
        assert!(bezier_move_point(&mut c, 4, &Vector { x: 5.0, y: 5.0, z: 0.0 }, &mut NoHost));
        assert_eq!((c.keys[1].prev, c.keys[1].next), (Vector { x: 3.0, y: 5.0, z: 0.0 }, Vector { x: 8.0, y: 5.0, z: 0.0 }));
        assert!(!bezier_move_point(&mut c, 6, &Vector { x: 0.0, y: 0.0, z: 0.0 }, &mut NoHost));
    }

    #[test]
    fn merges() {
        let a = Curve::from_points(LINEAR, false, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        let b = Curve::from_points(LINEAR, false, &[[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [7.0, 1.0, 0.0]]);
        let mut n = 0;
        let m = merge(&a, &b, 0.5, &mut n, &mut NoHost).unwrap();
        assert_eq!((m.nb(), n, m.keys[2].pos.x), (5, 1, 5.0));
        let pts: Vec<[f32; 3]> = (0..5).map(|i| [i as f32, 0.0, 0.0]).collect();
        let far: Vec<[f32; 3]> = (0..5).map(|i| [i as f32 + 10.0, 2.0, 0.0]).collect();
        let (a, b) = (Curve::from_points(BSPLINE, false, &pts), Curve::from_points(BSPLINE, false, &far));
        let m = merge(&a, &b, 0.5, &mut n, &mut NoHost).unwrap();
        assert_eq!(m.nb(), 7);
        assert_eq!(m.keys[2].pos, Vector { x: 6.0, y: 1.0, z: 0.0 }); // halfway between a[2] and b[0]
        assert_eq!(m.keys[5].pos.x, 13.0);
        let (a, b) = (Curve::from_points(BEZIER, false, &pts), Curve::from_points(BEZIER, false, &far));
        let (w, off) = bezier_merge(&a, &b, 2, &mut n, &mut NoHost).unwrap();
        assert_eq!((w.nb(), off), (9, Some(Vector { x: -6.0, y: -2.0, z: 0.0 })));
        assert_eq!(w.keys[5].pos, Vector { x: 5.0, y: 0.0, z: 0.0 });
        let mut closed = a.clone();
        closed.rec.closed = 1;
        assert!(bezier_merge(&closed, &b, 0, &mut n, &mut NoHost).is_none());
    }
}
