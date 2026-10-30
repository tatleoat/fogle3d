// SPDX-License-Identifier: MIT
//! Editing primitives: allocation and copies, points added, moved and removed, phantom
//! points, reversal, transformation.

use si_math::algebra::{matrix_copy, points_colinear};
use si_types::{Ptr32, Zero};
use si_x87::x;

use crate::{held, mv, new_key, Curve, Host, SplineKey, Vector, SPLINE_TOUCHED};

/// A key block for `count` keys [soft 0x0848f8dc SI_splineKeyAllocate]: room for `count`
/// rounded up to a multiple of 16 records, cleared; the first `count` then set to zero
/// vectors, the float 1.0 at +0x24 and zero constraint, segment and flag. The records past
/// `count` stay all zero (their float at +0x24 is 0, not 1). A count of 0 or less gives no
/// block (the original's allocator returns null for 0 records).
pub fn key_allocate(count: i32) -> Vec<SplineKey> {
    let room = count.wrapping_sub(((count.wrapping_sub(1) as u32 & 0xf) as i32).wrapping_sub(0xf));
    let mut keys = vec![SplineKey::zero(); room.max(0) as usize];
    for k in keys.iter_mut().take(count.max(0) as usize) {
        *k = new_key();
    }
    keys
}

/// The phantom point before `a` for a curve that goes on to `b` and `c`
/// [soft 0x0848e24c splineSetEnds]: `2a − b` when the three are on one line
/// (`SI_pointsColinear`, squared cross product below 1e-8), else `2a − float(2b − c)`.
/// Each coordinate is rounded to float once at the end; the inner term is rounded to float
/// first. Class: model.
pub fn set_ends(a: &Vector, b: &Vector, c: &Vector) -> Vector {
    if !points_colinear(&mv(*a), &mv(*b), &mv(*c)) {
        let t = |b: f32, c: f32| ((x(b) + b) - c).to_f32();
        let (tx, ty, tz) = (t(b.x, c.x), t(b.y, c.y), t(b.z, c.z));
        Vector { x: ((x(a.x) + a.x) - tx).to_f32(), y: ((x(a.y) + a.y) - ty).to_f32(), z: ((x(a.z) + a.z) - tz).to_f32() }
    } else {
        Vector { x: ((x(a.x) + a.x) - b.x).to_f32(), y: ((x(a.y) + a.y) - b.y).to_f32(), z: ((x(a.z) + a.z) - b.z).to_f32() }
    }
}

/// Recomputes the phantom end points [soft 0x0848df94 SI_splineSetDummy].
///
/// Only for a spline whose `phantom` word (+0x06) is set and that is not closed. A
/// B-spline or cardinal curve starts at its second key and ends at its last but one; the
/// first and last keys are "phantoms" placed so that the curve's ends behave:
///
/// - exactly 3 keys, type 2 or 3: all three keys take the middle key's position;
/// - 4 keys or more (the count read as a signed 16-bit number) and type 2, or exactly 4
///   keys and type 3: first = `2 k₁ − k₂`, last = `2 kₙ₋₂ − kₙ₋₃`;
/// - more than 4 keys and type 3: first = `2 k₁ − float(2 k₂ − k₃)`, last =
///   `2 kₙ₋₂ − float(2 kₙ₋₃ − kₙ₋₄)`;
/// - any other type: nothing.
///
/// Both handles of a recomputed key are set to its position. Class: model (two operations
/// before each store).
pub fn set_dummy(c: &mut Curve) {
    let nb = c.rec.nb_keys;
    if c.rec.phantom == 0 || c.rec.closed != 0 {
        return;
    }
    let kind = c.rec.kind;
    if nb == 3 && (kind == 2 || kind == 3) {
        if c.keys.len() < 3 {
            return;
        }
        let p = c.keys[1].pos;
        for i in [2, 0] {
            c.keys[i].pos = p;
            c.keys[i].next = p;
            c.keys[i].prev = p;
        }
        return;
    }
    if nb <= 3 {
        return;
    }
    let n = nb as usize;
    if c.keys.len() < n {
        return;
    }
    let simple = |a: Vector, b: Vector| Vector { x: ((x(a.x) + a.x) - b.x).to_f32(), y: ((x(a.y) + a.y) - b.y).to_f32(), z: ((x(a.z) + a.z) - b.z).to_f32() };
    let (first, last) = if kind == 2 || (nb == 4 && kind == 3) {
        (simple(c.keys[1].pos, c.keys[2].pos), simple(c.keys[n - 2].pos, c.keys[n - 3].pos))
    } else if kind == 3 {
        // the inner term is stored to a float before it is subtracted
        let first = simple(c.keys[1].pos, simple(c.keys[2].pos, c.keys[3].pos));
        // the first key is already replaced when the last is computed; with five keys or
        // more the last one's sources (n − 2, n − 3, n − 4) do not include it
        (first, simple(c.keys[n - 2].pos, simple(c.keys[n - 3].pos, c.keys[n - 4].pos)))
    } else {
        return;
    };
    for (i, p) in [(0, first), (n - 1, last)] {
        c.keys[i].pos = p;
        c.keys[i].next = p;
        c.keys[i].prev = p;
    }
}

/// Marks a spline as changed [soft 0x0848c924 SI_splineTouch]: the phantom points are
/// recomputed ([`set_dummy`]), bit 0x10 of the byte at +0x04 is set, the length table is
/// freed, the scene is told ([`Host::touched`]), and when the spline is "keyframed"
/// (+0x70 is 1: a percentage curve has keys tied to its points, finding 0017) the path keys
/// of the curve that follows it are brought to their points again and the curve editor is
/// refreshed.
pub fn touch(c: &mut Curve, host: &mut dyn Host) {
    set_dummy(c);
    c.rec.unk_04 |= SPLINE_TOUCHED;
    if !c.rec.lengths.is_null() {
        c.lengths = Vec::new();
        c.rec.lengths = Ptr32::NULL;
    }
    host.touched(c);
    if c.rec.unk_70 == 1 {
        let owner = host.find_path_fcurve_owner(c);
        host.update_fcurve_path_keys(owner, c);
        host.check_spline_to_fcurve(owner, c);
        host.refresh_fcurve(0);
    }
}

/// Copies the content of `from` into `to` [soft 0x0848aa9c SI_splineTransfer]: closed,
/// phantom, key count, a new key block with copies of the keys in use (whole records),
/// step, type, tension and the matrix. The name, the word at +0x04, the defaults for new
/// keys (+0x64, +0x66), the length table and the keyframed word of `to` stay as they are,
/// and its old key block is not freed (the original overwrites the pointer).
pub fn transfer(from: &Curve, to: &mut Curve) {
    to.rec.closed = from.rec.closed;
    to.rec.phantom = from.rec.phantom;
    to.rec.nb_keys = from.rec.nb_keys;
    to.keys = key_allocate(from.nb() as i32);
    to.rec.keys = if to.keys.is_empty() { Ptr32::NULL } else { held() };
    for i in 0..from.nb() {
        to.keys[i] = from.key(i);
    }
    to.rec.step = from.rec.step;
    to.rec.kind = from.rec.kind;
    to.rec.tension = from.rec.tension;
    to.rec.matrix = matrix_copy(&from.rec.matrix);
}

/// A copy of a spline [soft 0x0848aa74 SI_splineCopy]: a newly allocated spline (named
/// `spl<n>` from the allocator's counter, [`si_types::geom::Spline::allocate`]) that
/// [`transfer`] fills.
pub fn copy(from: &Curve, counter: &mut i32) -> Curve {
    let (rec, name) = crate::Spline::allocate(counter);
    let mut to = Curve { rec, name: Some(name), keys: Vec::new(), lengths: Vec::new() };
    to.rec.name = held();
    transfer(from, &mut to);
    to
}

/// Frees a spline and what it owns [soft 0x0848ab48 SI_splineDispose]: key block, length
/// table, name, record; the caller's pointer is cleared. Nothing for a null pointer.
pub fn dispose(c: &mut Option<Curve>) {
    *c = None;
}

/// Copies the three vectors of every key in use from `from` to `to`, leaving the rest of
/// each key [soft 0x08490664 SI_splineTransferPoints]. The count is `from`'s; the original
/// does not look at `to`'s (the port stops at the end of `to`'s block).
pub fn transfer_points(from: &Curve, to: &mut Curve) {
    for i in 0..from.nb().min(to.keys.len()) {
        let k = from.key(i);
        (to.keys[i].prev, to.keys[i].pos, to.keys[i].next) = (k.prev, k.pos, k.next);
    }
}

/// Moves point `index` [soft 0x0848ac34 SI_splineChangePoint]: position and both handles
/// take `p`. A phantom end point is left alone (the spline has `phantom` set, is open, and
/// `index` is its first or last key): [`touch`] recomputes it anyway. The spline is touched
/// in every case. The original does not check `index`; the port ignores one outside the
/// key block.
pub fn change_point(c: &mut Curve, index: i32, p: &Vector, host: &mut dyn Host) {
    let last = c.rec.nb_keys as u16 as i32 - 1;
    if c.rec.phantom == 0 || c.rec.closed != 0 || (index != 0 && index != last) {
        if let Some(k) = usize::try_from(index).ok().and_then(|i| c.keys.get_mut(i)) {
            (k.pos, k.prev, k.next) = (*p, *p, *p);
        }
    }
    touch(c, host);
}

/// Inserts a point before key `index` [soft 0x0848acc0 SI_splineAddPoint] and returns the
/// index it got (the original returns the key's address).
///
/// - Without a key block one for a single key is allocated and the index is 0. With one,
///   the block grows by room for 16 keys whenever the count is a multiple of 16 (the new
///   records are not initialised by the original; zero here).
/// - `index` is clamped to `[0, count]`; the keys from it on move up one record.
/// - The new key gets `p` as position and both handles, the spline's defaults (+0x64,
///   +0x66) as constraint and segment type, and flag 0. **Its float at +0x24 and its
///   padding are not set**: they keep what the record held (the key that used to be there,
///   or what the block held past the last key).
/// - A keyframed spline tells the path's percentage curve (`SI_addFcurvePathKey`; for the
///   types 2 and 3, whose first key is a phantom, with `index − 1`).
/// - The count goes up by one and the spline is touched.
pub fn add_point(c: &mut Curve, index: i32, p: &Vector, host: &mut dyn Host) -> usize {
    let mut index = index;
    if c.rec.keys.is_null() {
        c.keys = key_allocate(1);
        c.rec.keys = held();
        index = 0;
    } else if c.nb() % 16 == 0 {
        c.keys.resize(c.nb() + 16, SplineKey::zero());
    }
    let nb = c.rec.nb_keys as u16 as i32;
    index = index.min(nb).max(0);
    let mut i = nb - 1;
    while i >= index {
        // a record past the block is what the original would overrun; not written here
        if (i as usize + 1) < c.keys.len() {
            c.keys[i as usize + 1] = c.keys[i as usize];
        }
        if host.has_index() {
            host.set_index(i, i + 1);
        }
        i -= 1;
    }
    if let Some(k) = c.keys.get_mut(index as usize) {
        (k.next, k.pos, k.prev) = (*p, *p, *p);
        k.constraint = c.rec.unk_64;
        k.segment = c.rec.unk_66;
        k.flag = 0;
    }
    if c.rec.unk_70 == 1 {
        let at = if c.rec.kind == 2 || c.rec.kind == 3 { index - 1 } else { index };
        let owner = host.find_path_fcurve_owner(c);
        host.add_fcurve_path_key(owner, at);
    }
    c.rec.nb_keys = c.rec.nb_keys.wrapping_add(1);
    touch(c, host);
    index as usize
}

/// Removes every selected key (bit 0 of the key's flag)
/// [soft 0x0848ae20 SI_splineDeleteSelection]; returns whether any was. The keys that stay close up in
/// order; the records past the new count keep their old content. A keyframed spline's
/// percentage curve loses the path keys of each removed point (numbered as the points are
/// after the removals so far, less one for the types 2 and 3) when a curve was found. The
/// spline is touched only when something was removed.
pub fn delete_selection(c: &mut Curve, host: &mut dyn Host) -> bool {
    let mut removed = 0i32;
    let owner = if c.rec.unk_70 == 1 { host.find_path_fcurve_owner(c) } else { 0 };
    let mut kept = 0usize;
    for r in 0..c.nb() {
        let k = c.key(r);
        if k.flag & 1 == 0 {
            if kept < c.keys.len() {
                c.keys[kept] = k;
            }
            if host.has_index() {
                host.set_index(r as i32, kept as i32);
            }
            kept += 1;
        } else {
            if owner != 0 {
                let at = (r as i32 as i16).wrapping_sub(removed as i16);
                let at = if c.rec.kind == 2 || c.rec.kind == 3 { at.wrapping_sub(1) } else { at };
                host.delete_fcurve_path_key(owner, at);
            }
            if host.has_index() {
                host.set_index(r as i32, -1);
            }
            removed += 1;
        }
    }
    if removed != 0 {
        c.rec.nb_keys = c.rec.nb_keys.wrapping_sub(removed as i16);
        touch(c, host);
    }
    removed != 0
}

/// Removes key `index` [soft 0x0848af3c SI_splineDeletePoint]; false for an index outside
/// the keys in use. The later keys move down one record (the old last record keeps its
/// content). A keyframed spline's percentage curve is told first, without a check that a
/// curve was found.
pub fn delete_point(c: &mut Curve, index: i32, host: &mut dyn Host) -> bool {
    if index < 0 || c.rec.nb_keys as u16 as i32 <= index {
        return false;
    }
    if c.rec.unk_70 == 1 {
        let at = index as i16;
        let at = if c.rec.kind == 2 || c.rec.kind == 3 { at.wrapping_sub(1) } else { at };
        let owner = host.find_path_fcurve_owner(c);
        host.delete_fcurve_path_key(owner, at);
    }
    c.rec.nb_keys = c.rec.nb_keys.wrapping_sub(1);
    if host.has_index() {
        host.set_index(index, -1);
    }
    let mut i = index;
    while i < c.rec.nb_keys as u16 as i32 {
        let next = c.key(i as usize + 1);
        if let Some(k) = c.keys.get_mut(i as usize) {
            *k = next;
        }
        if host.has_index() {
            host.set_index(i + 1, i);
        }
        i += 1;
    }
    touch(c, host);
    true
}

/// Gives a B-spline or cardinal spline phantom end points [soft 0x0848e2f0 SI_allDummy]:
/// sets `phantom`, and adds points at the origin until there are three keys (one in front
/// when there are fewer than two, then one at the end when fewer than three). Other types
/// are left alone.
pub fn all_dummy(c: &mut Curve, host: &mut dyn Host) {
    if c.rec.kind == 2 || c.rec.kind == 3 {
        c.rec.phantom = 1;
        let zero = Vector { x: 0.0, y: 0.0, z: 0.0 };
        if (c.rec.nb_keys as u16) < 2 {
            add_point(c, 0, &zero, host);
        }
        if (c.rec.nb_keys as u16) < 3 {
            add_point(c, c.rec.nb_keys as u16 as i32, &zero, host);
        }
    }
}

/// Reverses the direction of a spline [soft 0x0848e35c SI_splineInverse]: the keys change
/// places end for end as whole records and each key's two handles change places. For a
/// Bézier spline the segment types then move up one key (the type at a key describes the
/// segment that *ends* there; the last key's goes to the first). A keyframed spline's
/// percentage curve has its path keys renumbered (`SI_invertFcurvePathKeys`). The spline is
/// touched.
///
/// A Bézier spline without keys makes the original read and write the two bytes before
/// and inside a key block that may not exist; the port does nothing there.
pub fn inverse(c: &mut Curve, host: &mut dyn Host) {
    let nb = c.rec.nb_keys as u16 as i32;
    if c.keys.len() >= nb as usize {
        for i in 0..(nb + 1) >> 1 {
            let (i, j) = (i as usize, (nb - 1 - i) as usize);
            let saved = c.keys[i];
            c.keys[i] = c.keys[j];
            c.keys[i].prev = c.keys[j].next;
            c.keys[i].next = c.keys[j].prev;
            if host.has_index() {
                host.set_index(j as i32, i as i32);
                host.set_index(i as i32, nb - (i as i32 + 1));
            }
            c.keys[j] = saved;
            c.keys[j].prev = saved.next;
            c.keys[j].next = saved.prev;
        }
        if c.rec.kind == 1 && nb > 0 {
            let last = c.keys[nb as usize - 1].segment;
            for k in (1..nb as usize).rev() {
                c.keys[k].segment = c.keys[k - 1].segment;
            }
            c.keys[0].segment = last;
        }
    }
    if c.rec.unk_70 == 1 {
        let owner = host.find_path_fcurve_owner(c);
        host.invert_fcurve_path_keys(owner, c);
    }
    touch(c, host);
}

/// A point times a matrix as the two transforming routines compute it: each coordinate
/// `float((((y × m[1][j] + x × m[0][j]) + z × m[2][j]) + m[3][j]))`, one rounding.
fn point_matrix(v: Vector, m: &si_math::types::Matrix) -> Vector {
    let at = |j: usize| (((x(v.y) * m[1][j] + x(v.x) * m[0][j]) + x(v.z) * m[2][j]) + m[3][j]).to_f32();
    Vector { x: at(0), y: at(1), z: at(2) }
}

/// Transforms the keys in use by a matrix [soft 0x08494138 SI_splineTransform]: positions,
/// and for a Bézier spline both handles too (for the other types the handles are left
/// behind). Nothing is touched. Class: model.
pub fn transform(c: &mut Curve, m: &si_math::types::Matrix) {
    let bezier = c.rec.kind == 1;
    let n = c.nb().min(c.keys.len());
    for k in &mut c.keys[..n] {
        if bezier {
            k.prev = point_matrix(k.prev, m);
        }
        k.pos = point_matrix(k.pos, m);
        if bezier {
            k.next = point_matrix(k.next, m);
        }
    }
}

/// [`transform`] followed by [`touch`] [soft 0x0848e568 SI_splineMultiplyDefinition]: what
/// `SI_modelMultiplyDef` calls for a spline node (finding 0015). Class: model.
pub fn multiply_definition(c: &mut Curve, m: &si_math::types::Matrix, host: &mut dyn Host) {
    transform(c, m);
    touch(c, host);
}

/// The type a name stands for [soft 0x0848c348 si_splineGetType]: `LINEAR` 0, `BEZIER` 1,
/// `BSPLINE` 2, and anything else 3 (cardinal). The comparison is exact and takes the
/// whole string.
pub fn get_type(name: &[u8]) -> i32 {
    match name {
        b"LINEAR" => 0,
        b"BEZIER" => 1,
        b"BSPLINE" => 2,
        _ => 3,
    }
}

/// One of the three vectors of a Bézier key by "control point number"
/// [soft 0x084305e8 SI_bezierGetPoint] (`bezier.c`): number `p` belongs to key `p / 3` and is its `prev`
/// handle (remainder 0), its position (1) or its `next` handle (2). `None` where the
/// original leaves the destination alone: a negative number that is not a multiple of 3
/// (C's remainder is then negative). The original does not check the key index; the port
/// reads a zero key outside the block.
pub fn bezier_get_point(c: &Curve, p: i32) -> Option<Vector> {
    let k = if p / 3 < 0 { SplineKey::zero() } else { c.key((p / 3) as usize) };
    match p % 3 {
        0 => Some(k.prev),
        1 => Some(k.pos),
        2 => Some(k.next),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoHost, BEZIER, BSPLINE, CARDINAL, LINEAR};

    fn v(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    #[test]
    fn key_blocks_come_in_sixteens() {
        for (n, room) in [(0, 0), (1, 16), (16, 16), (17, 32), (-3, 0)] {
            assert_eq!(key_allocate(n).len(), room, "{n}");
        }
        let k = key_allocate(2);
        assert_eq!((crate::key_weight(&k[1]), crate::key_weight(&k[2])), (1.0, 0.0));
    }

    #[test]
    fn phantom_points_mirror_the_ends() {
        let pts: Vec<[f32; 3]> = (0..6).map(|i| [(i * i) as f32, i as f32, 0.0]).collect();
        let mut c = Curve::from_points(BSPLINE, false, &pts);
        c.rec.phantom = 1;
        set_dummy(&mut c);
        assert_eq!(c.keys[0].pos, v(2.0 * 1.0 - 4.0, 2.0 - 2.0, 0.0));
        assert_eq!(c.keys[5].pos, v(2.0 * 16.0 - 9.0, 8.0 - 3.0, 0.0));
        assert_eq!(c.keys[0].next, c.keys[0].pos);
        let mut c = Curve::from_points(CARDINAL, false, &pts);
        c.rec.phantom = 1;
        set_dummy(&mut c);
        assert_eq!(c.keys[0].pos.x, 2.0 * 1.0 - (2.0 * 4.0 - 9.0));
        let mut c = Curve::from_points(CARDINAL, false, &pts[..3]);
        c.rec.phantom = 1;
        set_dummy(&mut c);
        assert_eq!((c.keys[0].pos, c.keys[2].pos), (c.keys[1].pos, c.keys[1].pos));
        // collinear points take the short form
        assert_eq!(set_ends(&v(1.0, 0.0, 0.0), &v(2.0, 0.0, 0.0), &v(5.0, 0.0, 0.0)), v(0.0, 0.0, 0.0));
        assert_eq!(set_ends(&v(1.0, 0.0, 0.0), &v(2.0, 0.0, 0.0), &v(5.0, 1.0, 0.0)), v(3.0, 1.0, 0.0));
    }

    #[test]
    fn points_are_added_and_removed() {
        let mut c = Curve::new();
        assert_eq!(add_point(&mut c, 7, &v(1.0, 0.0, 0.0), &mut NoHost), 0);
        assert_eq!(add_point(&mut c, 7, &v(3.0, 0.0, 0.0), &mut NoHost), 1);
        assert_eq!(add_point(&mut c, 1, &v(2.0, 0.0, 0.0), &mut NoHost), 1);
        assert_eq!(c.used().iter().map(|k| k.pos.x).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
        assert_eq!((c.keys[1].constraint, c.keys[1].segment, c.keys.len()), (2, 1, 16));
        // the first key came from the allocator, the others from cleared records
        assert_eq!((crate::key_weight(&c.keys[0]), crate::key_weight(&c.keys[2])), (1.0, 0.0));
        c.keys[1].flag = 1;
        assert!(delete_selection(&mut c, &mut NoHost));
        assert!(!delete_selection(&mut c, &mut NoHost));
        assert!(delete_point(&mut c, 0, &mut NoHost) && !delete_point(&mut c, 1, &mut NoHost));
        assert_eq!(c.used().iter().map(|k| k.pos.x).collect::<Vec<_>>(), [3.0]);
        let mut c = Curve::from_points(BSPLINE, false, &[]);
        all_dummy(&mut c, &mut NoHost);
        assert_eq!((c.rec.nb_keys, c.rec.phantom), (2, 1)); // the second test sees the first add
        for i in 0..15 {
            add_point(&mut c, 99, &v(i as f32, 0.0, 0.0), &mut NoHost);
        }
        assert_eq!((c.nb(), c.keys.len()), (17, 32));
    }

    #[test]
    fn reversal_swaps_handles_and_shifts_segment_types() {
        let mut c = Curve::from_points(BEZIER, false, &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        for (i, k) in c.keys.iter_mut().take(3).enumerate() {
            k.prev.y = -1.0 - i as f32;
            k.next.y = 1.0 + i as f32;
            k.segment = 10 + i as i16;
        }
        inverse(&mut c, &mut NoHost);
        assert_eq!(c.used().iter().map(|k| k.pos.x).collect::<Vec<_>>(), [2.0, 1.0, 0.0]);
        assert_eq!((c.keys[0].prev.y, c.keys[0].next.y), (3.0, -3.0));
        assert_eq!((c.keys[1].prev.y, c.keys[1].next.y), (2.0, -2.0));
        assert_eq!(c.used().iter().map(|k| k.segment).collect::<Vec<_>>(), [10, 12, 11]);
        let mut l = Curve::from_points(LINEAR, false, &[[1.0, 2.0, 3.0]]);
        let m = [[2.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [1.0, 1.0, 1.0, 1.0]];
        multiply_definition(&mut l, &m, &mut NoHost);
        assert_eq!((l.keys[0].pos, l.keys[0].prev), (v(3.0, 5.0, 7.0), v(1.0, 2.0, 3.0)));
        assert_eq!((get_type(b"BEZIER"), get_type(b"BSPLINE"), get_type(b"LINEAR"), get_type(b"bezier")), (1, 2, 0, 3));
    }
}
