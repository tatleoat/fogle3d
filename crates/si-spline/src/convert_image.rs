// SPDX-License-Identifier: MIT
//! The conversion of a spline to a polyline, its clean-up, its reversal and the removal of
//! selected keys, on the one memory image (finding 0020, sections 5, 6 and 26; finding
//! 0024): what `face.c` calls when a face is made of straight pieces (the triangulated
//! save, finding 0015) and what the model package calls to reverse a spline.
//!
//! Each function takes the image and the arguments the original takes, reads and writes
//! the records where they lie (a face's outline is a spline record inside the face) and
//! makes the original's allocation calls in its order. The arithmetic is the value forms'
//! ([`crate::convert`], [`crate::edit`]), which were compared first; what is added here is
//! where every operand is read and every result written, including the writes the
//! original makes outside the keys in use.

use si_math::algebra::{matrix_multiply, points_colinear, vector_normalize};
use si_math::types::Matrix;
use si_mem::{Mem, Ptr32};
use si_types::geom::{Spline, SplineKey, Vector};
use si_x87::{fmov, x, F80};

use crate::convert::{FAR, NEAR};
use crate::basis::{matrix_precision, spline_basis_matrix};
use crate::eval::{ONE_THIRD, TWO_THIRDS};
use crate::intersect_image::spline_copy;
use crate::spline_image::{spline_add_point, spline_allocate, spline_key_allocate, spline_touch, spline_update_matrix, Callee, Outside, SI_DELETE_FCURVE_PATH_KEY, SI_FIND_PATH_FCURVE_OWNER, SI_GBL_INDEX};

/// `SI_invertFcurvePathKeys` (0x0821fd10): renumbers the path keys of the percentage curve
/// that follows a reversed spline. Called with three words: the curve, the spline and 0
/// (its third parameter, an `SI_NurbsDir *`, is null for a spline).
pub const SI_INVERT_FCURVE_PATH_KEYS: Callee = Callee { name: "SI_invertFcurvePathKeys", addr: 0x0821_fd10 };

/// The size of a key record.
const KEY: u32 = SplineKey::SIZE as u32;

fn key_at(keys: Ptr32<SplineKey>, i: i32) -> u32 {
    keys.addr().wrapping_add((i as u32).wrapping_mul(KEY))
}

fn nb(m: &Mem, s: Ptr32<Spline>) -> i32 {
    m[s].nb_keys as u16 as i32
}

fn pos_of(m: &Mem, k: u32) -> Vector {
    Vector { x: m.f32(k + 0x0c), y: m.f32(k + 0x10), z: m.f32(k + 0x14) }
}

/// Removes every key whose flag has bit 0 set [soft 0x0848ae20 SI_splineDeleteSelection];
/// 1 when one was removed, else 0.
///
/// A keyframed spline (+0x70 is 1) asks `SI_findPathFcurveOwner` first. Then for each key
/// in use, in order: a selected one, when a curve was found, is taken off it
/// (`SI_deleteFcurvePathKey(curve, n)` with n the key's number less the keys removed so
/// far, less one more for the types 2 and 3, as a 16-bit number) and gets −1 in
/// `SI_gblIndex`; any other is copied whole (12 words, onto itself while nothing was
/// removed) to the next free record and gets that record's number in the table. The count
/// is lowered by the keys removed (16 bits) and the spline touched, only when there were
/// any. The records past the new count keep their content.
pub fn spline_delete_selection(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>) -> i32 {
    let mut removed = 0i32;
    let mut owner = 0u32;
    if m[s].unk_70 == 1 {
        owner = o.call(m, &SI_FIND_PATH_FCURVE_OWNER, &[s.addr()]);
    }
    let mut kept = 0i32;
    if removed < nb(m, s) {
        let mut r = 0i32;
        loop {
            let keys = m[s].keys;
            if m.u8(key_at(keys, r) + 0x2c) & 1 != 0 {
                if owner != 0 {
                    let mut at = r - removed;
                    if (m[s].kind as u32).wrapping_sub(2) <= 1 {
                        at -= 1;
                    }
                    o.call(m, &SI_DELETE_FCURVE_PATH_KEY, &[owner, at as i16 as i32 as u32]);
                }
                let table = m[SI_GBL_INDEX];
                if !table.is_null() {
                    m[table.add(r)] = -1;
                }
                removed += 1;
            } else {
                // rep movs of 12 words, upwards: the destination is never above the source
                for w in 0..12u32 {
                    let v = m.u32(key_at(keys, r) + 4 * w);
                    m.set_u32(key_at(keys, kept) + 4 * w, v);
                }
                let table = m[SI_GBL_INDEX];
                if !table.is_null() {
                    m[table.add(r)] = kept;
                }
                kept += 1;
            }
            r += 1;
            if r >= nb(m, s) {
                break;
            }
        }
    }
    if removed != 0 {
        m[s].nb_keys = m[s].nb_keys.wrapping_sub(removed as i16);
        spline_touch(m, o, s);
    }
    (removed != 0) as i32
}

/// Writes a key of a polyline: position, `next` handle and `prev` handle the point, flag 0.
fn put_point(m: &mut Mem, k: u32, f: &[f32; 4]) {
    m.set_f32(k + 0x0c, fmov(f[0]));
    m.set_f32(k + 0x10, fmov(f[1]));
    m.set_f32(k + 0x14, fmov(f[2]));
    // the handles by integer moves: next from the position, prev from next
    for w in 0..3u32 {
        let v = m.u32(k + 0x0c + 4 * w);
        m.set_u32(k + 0x18 + 4 * w, v);
    }
    for w in 0..3u32 {
        let v = m.u32(k + 0x18 + 4 * w);
        m.set_u32(k + 4 * w, v);
    }
    m.set_i16(k + 0x2c, 0);
}

/// A linear spline through `steps` points a segment [soft 0x0848c3ac splineConvertToLinearRegular];
/// the arithmetic and the counts are [`crate::convert::convert_to_linear_regular`]'s.
///
/// The segments are `keys + closed − 1`, less 2 for an open spline of type 2 or 3.
/// `SI_splineAllocate` (null ends the routine); the closed word; the key block
/// `SI_splineKeyAllocate(steps × segments + 1)`; step 1. The basis goes into a local by
/// `SI_splineBasisMatrix`, which leaves it as it was for a type outside 0..=3: the
/// original then works on whatever its stack held, zeros here. Per segment the geometry
/// (with a fourth column of 1.0) is the four positions from the segment's first key on,
/// numbers taken modulo the count, or the Bézier segment's control points, the following
/// key's straight-segment mark making the inner two the thirds of the chord; the start
/// point is written only into an empty open result; each step's point goes to the record
/// at the count, **whether or not the block has room for it or exists**: an open source
/// with segments and a block of no keys makes the original write through null. Ends with
/// `SI_splineUpdateMatrix` of the result.
pub fn spline_convert_to_linear_regular(m: &mut Mem, s: Ptr32<Spline>, steps: i32) -> Ptr32<Spline> {
    let count = nb(m, s);
    let closed = m[s].closed;
    let mut segs = count + closed as i32 - 1;
    if closed == 0 && (m[s].kind as u32).wrapping_sub(2) <= 1 {
        segs -= 2;
    }
    let out = spline_allocate(m);
    if out.is_null() {
        return out;
    }
    m[out].closed = m[s].closed;
    let block = spline_key_allocate(m, steps.wrapping_mul(segs).wrapping_add(1));
    m[out].keys = block;
    m[out].step = 1;
    // the original's local is uninitialised; zeros stand for it (a type outside 0..=3)
    let mut basis: Matrix = [[0.0; 4]; 4];
    spline_basis_matrix(m[s].kind, fmov(m[s].tension), &mut basis);
    let d = matrix_multiply(&matrix_precision(steps), &basis);
    let mut g: Matrix = [[0.0, 0.0, 0.0, 1.0]; 4];
    let mut from = m[s].keys;
    let mut first = 0i32;
    for seg in 0..segs.max(0) {
        if m[s].kind != 1 {
            for (k, row) in g.iter_mut().enumerate() {
                let i = (first + k as i32) % nb(m, s);
                let p = pos_of(m, key_at(m[s].keys, i));
                row[..3].copy_from_slice(&[fmov(p.x), fmov(p.y), fmov(p.z)]);
            }
        } else {
            let to: Ptr32<SplineKey> = if seg == nb(m, s) - 1 { m[s].keys } else { m[s].keys.add(seg + 1) };
            let (k0, k1) = (pos_of(m, from.addr()), pos_of(m, to.addr()));
            g[0][..3].copy_from_slice(&[fmov(k0.x), fmov(k0.y), fmov(k0.z)]);
            g[3][..3].copy_from_slice(&[fmov(k1.x), fmov(k1.y), fmov(k1.z)]);
            if m.i16(to.addr() + 0x2a) == crate::SEGMENT_LINEAR {
                let mix = |a: f32, b: f32, wa: f64, wb: f64| (x(wa) * a + x(wb) * b).to_f32();
                let (a, b) = ([k0.x, k0.y, k0.z], [k1.x, k1.y, k1.z]);
                for j in 0..3 {
                    g[1][j] = mix(a[j], b[j], TWO_THIRDS, ONE_THIRD);
                    g[2][j] = mix(a[j], b[j], ONE_THIRD, TWO_THIRDS);
                }
            } else {
                let next = m[from].next;
                let prev = m[to].prev;
                g[1][..3].copy_from_slice(&[fmov(next.x), fmov(next.y), fmov(next.z)]);
                g[2][..3].copy_from_slice(&[fmov(prev.x), fmov(prev.y), fmov(prev.z)]);
            }
            from = to;
        }
        first += 1;
        let mut f = matrix_multiply(&d, &g);
        if m[out].nb_keys == 0 && m[out].closed == 0 {
            let k = m[out].keys.addr();
            put_point(m, k, &f[3]);
            m[out].nb_keys = m[out].nb_keys.wrapping_add(1);
        }
        for _ in 0..steps.max(0) {
            for r in (1..4).rev() {
                for j in 0..4 {
                    f[r][j] = (x(f[r][j]) + f[r - 1][j]).to_f32();
                }
            }
            let k = key_at(m[out].keys, nb(m, out));
            put_point(m, k, &f[3]);
            m[out].nb_keys = m[out].nb_keys.wrapping_add(1);
        }
    }
    spline_update_matrix(m, out);
    out
}

/// A linear spline with as few points as a tolerance allows
/// [soft 0x0848e928 splineConvertToLinearFlatness]; the thinning is
/// [`crate::convert::convert_to_linear_flatness`]'s.
///
/// `float(tolerance²)`; [`spline_convert_to_linear_regular`] at 100 steps (its result is
/// not tested: null makes the original read through null); the points are marked by
/// setting bit 0 of their flag byte, the last one too on a closed result (`keys + count ×
/// 0x30 − 4`, the four bytes before the block when there is no point), and removed by
/// [`spline_delete_selection`].
pub fn spline_convert_to_linear_flatness(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, tolerance: f32) -> Ptr32<Spline> {
    let tol2 = (x(tolerance) * tolerance).to_f32();
    let out = spline_convert_to_linear_regular(m, s, 100);
    let mut base = 0i32;
    let fpos = |m: &Mem, i: i32| {
        let p = pos_of(m, key_at(m[out].keys, i));
        Vector { x: fmov(p.x), y: fmov(p.y), z: fmov(p.z) }
    };
    let mut anchor = fpos(m, 0);
    let mut i = 1i32;
    while i < nb(m, out) - 1 {
        let to = pos_of(m, key_at(m[out].keys, i + 1));
        let mut dir = si_math::types::Vector::new((x(to.x) - anchor.x).to_f32(), (x(to.y) - anchor.y).to_f32(), (x(to.z) - anchor.z).to_f32());
        vector_normalize(&mut dir);
        let first = base + 1;
        let mut beyond = false;
        let mut j = first;
        while j <= i {
            let p = pos_of(m, key_at(m[out].keys, j));
            let (vx, vy, vz) = (x(p.x) - anchor.x, x(p.y) - anchor.y, x(p.z) - anchor.z);
            let cx = vy * dir.z - vz * dir.y;
            let cy = vz * dir.x - vx * dir.z;
            let cz = vx * dir.y - vy * dir.x;
            if (cz * cz + (cy * cy + cx * cx)).gt(x(tol2)) {
                beyond = true;
                break;
            }
            j += 1;
        }
        if beyond {
            for k in first..i {
                m.or_u8(key_at(m[out].keys, k) + 0x2c, 1);
            }
            base = i;
            anchor = fpos(m, base);
        }
        i += 1;
    }
    let mut j = base + 1;
    while j < nb(m, out) - 1 {
        m.or_u8(key_at(m[out].keys, j) + 0x2c, 1);
        j += 1;
    }
    if m[out].closed != 0 {
        let at = key_at(m[out].keys, nb(m, out)).wrapping_sub(4);
        m.or_u8(at, 1);
    }
    spline_delete_selection(m, o, out);
    out
}

/// A linear spline that follows a spline [soft 0x0848beec SI_splineConvertToLinear]; null
/// for a spline without keys (the count tested as 16 bits). A linear spline: its copy
/// ([`spline_copy`]). `regular` not 0: [`spline_convert_to_linear_regular`] with `steps`,
/// the spline's own step (unsigned 16 bits) when `steps` is negative, 1 when that is 0.
/// Else [`spline_convert_to_linear_flatness`] with the tolerance made positive and
/// replaced by the float 0x38d1b717 when below the double 0.0001.
///
/// What `SI_faceConvertToLinear` calls (`regular` 1, the face's step, tolerance 0.0).
pub fn spline_convert_to_linear(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, regular: i16, steps: i32, tolerance: f32) -> Ptr32<Spline> {
    if m[s].nb_keys == 0 {
        return Ptr32::NULL;
    }
    if m[s].kind == 0 {
        return spline_copy(m, s);
    }
    if regular != 0 {
        let mut steps = steps;
        if steps < 0 {
            steps = m[s].step as u16 as i32;
        }
        if steps == 0 {
            steps = 1;
        }
        return spline_convert_to_linear_regular(m, s, steps);
    }
    let mut t = x(tolerance);
    if F80::ZERO.gt(t) {
        t = -t;
    }
    if t.lt(x(1e-4f64)) {
        t = x(f32::from_bits(0x38d1_b717));
    }
    spline_convert_to_linear_flatness(m, o, s, t.to_f32())
}

fn colinear(a: Vector, b: Vector, c: Vector) -> bool {
    points_colinear(&crate::mv(a), &crate::mv(b), &crate::mv(c))
}

/// Removes the superfluous points of a linear spline of more than two keys (the count as
/// 16 bits unsigned) [soft 0x0848eb28 SI_splineCleanup]; other splines are left alone. The
/// rule is [`crate::convert::cleanup`]'s: bit 0 of every flag byte cleared; each interior
/// key marked when `SI_pointsColinear(key, last kept, next)`; a closed spline's first key
/// tested between the last unmarked key and the first unmarked key from 1 on, its last key
/// between the last unmarked key before it and the first unmarked key; the marked keys
/// removed by [`spline_delete_selection`].
pub fn spline_cleanup(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>) {
    if m[s].kind != 0 || (m[s].nb_keys as u16) <= 2 {
        return;
    }
    let mut i = 0;
    while i < nb(m, s) {
        let k = key_at(m[s].keys, i) + 0x2c;
        let v = m.u8(k) & 0xfe;
        m.set_u8(k, v);
        i += 1;
    }
    let keys = m[s].keys;
    let mut a = pos_of(m, key_at(keys, 0));
    let mut b = pos_of(m, key_at(keys, 1));
    let mut i = 1;
    while i < nb(m, s) - 1 {
        let next = pos_of(m, key_at(m[s].keys, i + 1));
        if colinear(b, a, next) {
            m.or_u8(key_at(m[s].keys, i) + 0x2c, 1);
        } else {
            a = b;
        }
        b = next;
        i += 1;
    }
    if m[s].closed == 0 {
        spline_delete_selection(m, o, s);
        return;
    }
    let selected = |m: &Mem, i: i32| m.u8(key_at(m[s].keys, i) + 0x2c) & 1 != 0;
    // the first key, between the last unmarked key and the first unmarked one from 1 on
    let mut before = nb(m, s) - 1;
    while before >= 0 && selected(m, before) {
        before -= 1;
    }
    if before >= 0 {
        let a = pos_of(m, key_at(m[s].keys, before));
        let b = pos_of(m, key_at(m[s].keys, 0));
        let mut after = 1;
        while after < nb(m, s) && selected(m, after) {
            after += 1;
        }
        if after < nb(m, s) {
            let next = pos_of(m, key_at(m[s].keys, after));
            if colinear(b, a, next) {
                m.or_u8(key_at(m[s].keys, 0) + 0x2c, 1);
            }
        }
    }
    // the last key, between the last unmarked key before it and the first unmarked one
    let mut before = nb(m, s) - 2;
    while before >= 0 && selected(m, before) {
        before -= 1;
    }
    if before >= 0 {
        let a = pos_of(m, key_at(m[s].keys, before));
        let b = pos_of(m, key_at(m[s].keys, nb(m, s) - 1));
        let mut after = 0;
        while after < nb(m, s) && selected(m, after) {
            after += 1;
        }
        if after < nb(m, s) {
            let next = pos_of(m, key_at(m[s].keys, after));
            if colinear(b, a, next) {
                let at = key_at(m[s].keys, nb(m, s)).wrapping_sub(4);
                m.or_u8(at, 1);
            }
        }
    }
    spline_delete_selection(m, o, s);
}

fn copy3(m: &mut Mem, to: u32, from: u32) {
    for w in 0..3u32 {
        let v = m.u32(from + 4 * w);
        m.set_u32(to + 4 * w, v);
    }
}

/// Reverses a spline [soft 0x0848e35c SI_splineInverse]: for i below `(count + 1) / 2`
/// with j = count − 1 − i, key i is saved (12 words), key j copied over it whole, its
/// `prev` handle set from key j's `next` and its `next` from key j's `prev` (for the middle
/// key that is the handle just written), the table `SI_gblIndex` (when set) gets
/// `[j] = i` and `[i] = count − (i + 1)`, and key j becomes the saved key with its handles
/// exchanged. A Bézier spline's segment words (+0x2a) then move up one key, the last
/// one's to the first: with no key the original reads the two bytes six before the block
/// and writes them into the first record. A keyframed spline:
/// `SI_invertFcurvePathKeys(SI_findPathFcurveOwner(spline), spline, 0)`. Touched at the end.
pub fn spline_inverse(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>) {
    let mut i = 0i32;
    while i < (nb(m, s) + 1) >> 1 {
        let saved: SplineKey = m.read(Ptr32::new(key_at(m[s].keys, i)));
        let j = nb(m, s) - 1 - i;
        let (ki, kj) = (key_at(m[s].keys, i), key_at(m[s].keys, j));
        let whole: SplineKey = m.read(Ptr32::new(kj));
        m.write(Ptr32::new(ki), whole);
        copy3(m, ki, kj + 0x18);
        copy3(m, ki + 0x18, kj);
        let table = m[SI_GBL_INDEX];
        if !table.is_null() {
            let j = nb(m, s) - 1 - i;
            m[table.add(j)] = i;
            let (table, n) = (m[SI_GBL_INDEX], nb(m, s));
            m[table.add(i)] = n - (i + 1);
        }
        let kj = key_at(m[s].keys, nb(m, s) - 1 - i);
        m.write(Ptr32::new(kj), saved);
        m.write(Ptr32::<Vector>::new(kj), saved.next);
        m.write(Ptr32::<Vector>::new(kj + 0x18), saved.prev);
        i += 1;
    }
    if m[s].kind == 1 {
        let count = nb(m, s);
        let last = m.u16(key_at(m[s].keys, count).wrapping_sub(6));
        let mut k = count - 1;
        while k > 0 {
            let keys = m[s].keys;
            let v = m.u16(key_at(keys, k) - 6);
            m.set_u16(key_at(keys, k) + 0x2a, v);
            k -= 1;
        }
        let first = m[s].keys.addr() + 0x2a;
        m.set_u16(first, last);
    }
    if m[s].unk_70 == 1 {
        let owner = o.call(m, &SI_FIND_PATH_FCURVE_OWNER, &[s.addr()]);
        o.call(m, &SI_INVERT_FCURVE_PATH_KEYS, &[owner, s.addr(), 0]);
    }
    spline_touch(m, o, s);
}

/// Gives a B-spline or cardinal spline its phantom end points [soft 0x0848e2f0 SI_allDummy]:
/// the phantom word set to 1, then with the zero vector in a local, when the count (16
/// bits unsigned) is at most 1 [`spline_add_point`] at 0, and when it is then at most 2 at
/// the count. Other types are left alone. What the model reader calls for a spline whose
/// phantom word is set.
pub fn all_dummy(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>) {
    if (m[s].kind as u32).wrapping_sub(2) > 1 {
        return;
    }
    m[s].phantom = 1;
    let zero = m.local(12);
    m.set_u32(zero, 0);
    m.set_u32(zero + 4, 0);
    m.set_u32(zero + 8, 0);
    if (m[s].nb_keys as u16) <= 1 {
        spline_add_point(m, o, s, 0, zero);
    }
    if (m[s].nb_keys as u16) <= 2 {
        let at = m[s].nb_keys as u16 as i32;
        spline_add_point(m, o, s, at, zero);
    }
    m.release(12);
}

/// Multiplies a spline's keys by a matrix [soft 0x0848e568 SI_splineMultiplyDefinition]
/// (`float (*)[3]`, rows of four: `mat` is the address of 16 floats), then
/// [`spline_touch`]. Unlike [`crate::edit::transform`] the loop runs over the key count (16
/// bits unsigned) without looking at how many keys the block holds, as a do-while after
/// the test of a nonzero count; a type 1 spline has all three vectors of a key moved (the
/// previous handle at +0x00, the position at +0x0c, the next handle at +0x18), any other
/// type the position only. A vector `(x, y, z)` becomes `((x m0 + y m4) + z m8) + m12`,
/// then the same with `m1 m5 m9 m13` and `m2 m6 m10 m14`, each in x87 extended precision
/// (the three inputs are loaded once, before the first store; a result is rounded to
/// `float` where stored). A key block that does not exist is read through whatever the
/// keys pointer holds: the read is the recorded fault and the loop ends there.
pub fn spline_multiply_definition(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, mat: u32) {
    let first = if m[s].kind == 1 { 0 } else { 0x0c };
    let last = if m[s].kind == 1 { 0x18 } else { 0x0c };
    let mut at = 0u32;
    let mut i = 0i32;
    if nb(m, s) != 0 {
        loop {
            let mut off = first;
            while off <= last {
                let keys = m[s].keys.addr();
                let a = keys.wrapping_add(at).wrapping_add(off);
                let (px, py, pz) = (m.f32(a), m.f32(a + 4), m.f32(a + 8));
                for c in 0..3u32 {
                    let (c0, c1, c2, c3) = (m.f32(mat + 4 * c), m.f32(mat + 16 + 4 * c), m.f32(mat + 32 + 4 * c), m.f32(mat + 48 + 4 * c));
                    let v = (x(px) * c0 + x(py) * c1 + x(pz) * c2 + c3).to_f32();
                    let keys = m[s].keys.addr();
                    m.set_f32(keys.wrapping_add(at).wrapping_add(off) + 4 * c, v);
                }
                off += 0x0c;
            }
            if m.fault().is_some() {
                break;
            }
            i += 1;
            at = at.wrapping_add(KEY);
            if i >= nb(m, s) {
                break;
            }
        }
    }
    spline_touch(m, o, s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spline_image::{init, NoOutside, SI_GRAF_LISTS};

    fn world() -> Mem {
        let mut m = Mem::new();
        init(&mut m);
        let lists = m.calloc(1, 0x24);
        m[SI_GRAF_LISTS] = lists;
        m
    }

    fn spline(m: &mut Mem, kind: i32, closed: i16, pts: &[[f32; 3]]) -> Ptr32<Spline> {
        let s = spline_allocate(m);
        let keys = spline_key_allocate(m, pts.len() as i32);
        m[s].keys = keys;
        m[s].nb_keys = pts.len() as i16;
        m[s].kind = kind;
        m[s].closed = closed;
        m[s].tension = 0.5;
        for (i, p) in pts.iter().enumerate() {
            let v = Vector { x: p[0], y: p[1], z: p[2] };
            let k = keys.add(i as i32);
            m[k].prev = v;
            m[k].pos = v;
            m[k].next = v;
        }
        spline_update_matrix(m, s);
        s
    }

    fn positions(m: &Mem, s: Ptr32<Spline>) -> Vec<(f32, f32)> {
        (0..nb(m, s)).map(|i| (m[m[s].keys.add(i)].pos.x, m[m[s].keys.add(i)].pos.y)).collect()
    }

    #[test]
    fn the_image_forms_agree_with_the_value_forms() {
        let pts: Vec<[f32; 3]> = (0..7).map(|i| [i as f32, ((i * i) % 5) as f32, 0.25 * i as f32]).collect();
        for (kind, closed) in [(1, 0), (2, 1), (3, 0), (2, 0)] {
            let mut m = world();
            let s = spline(&mut m, kind, closed, &pts);
            let value = crate::intersect_image::load_curve(&m, s);
            let mut counter = m[crate::spline_image::SPLINE_COUNT];
            let want = crate::convert::convert_to_linear(&value, 1, 4, 0.0, &mut counter, &mut crate::NoHost).unwrap();
            let got = spline_convert_to_linear(&mut m, &mut NoOutside, s, 1, 4, 0.0);
            assert_eq!(nb(&m, got) as usize, want.nb(), "{kind} {closed}");
            for (i, k) in want.used().iter().enumerate() {
                assert_eq!(m[m[got].keys.add(i as i32)].pos, k.pos);
            }
            let want = crate::convert::convert_to_linear(&value, 0, 0, 0.05, &mut counter, &mut crate::NoHost).unwrap();
            let got = spline_convert_to_linear(&mut m, &mut NoOutside, s, 0, 0, 0.05);
            assert_eq!(nb(&m, got) as usize, want.nb(), "{kind} {closed}");
            for (i, k) in want.used().iter().enumerate() {
                assert_eq!(m[m[got].keys.add(i as i32)].pos, k.pos);
            }
            assert!(m.fault().is_none());
        }
    }

    #[test]
    fn cleanup_and_reversal_in_place() {
        let mut m = world();
        let s = spline(&mut m, 0, 1, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0], [0.0, 1.0, 0.0]]);
        spline_cleanup(&mut m, &mut NoOutside, s);
        assert_eq!(positions(&m, s), [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let b = spline(&mut m, 1, 0, &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        for i in 0..3 {
            let k = m[b].keys.add(i);
            m[k].prev.y = -1.0 - i as f32;
            m[k].next.y = 1.0 + i as f32;
            m[k].segment = 10 + i as i16;
        }
        spline_inverse(&mut m, &mut NoOutside, b);
        assert_eq!(positions(&m, b), [(2.0, 0.0), (1.0, 0.0), (0.0, 0.0)]);
        let k0 = m[b].keys;
        assert_eq!((m[k0].prev.y, m[k0].next.y, m[k0.add(1)].prev.y), (3.0, -3.0, 2.0));
        assert_eq!((m[k0].segment, m[k0.add(1)].segment, m[k0.add(2)].segment), (10, 12, 11));
        let c = spline(&mut m, 2, 0, &[]);
        all_dummy(&mut m, &mut NoOutside, c);
        assert_eq!((m[c].nb_keys, m[c].phantom), (2, 1));
        assert!(m.fault().is_none());
    }
}

/// The control points of a Bézier spline as plain keys, on the image [soft 0x0848ff8c SI_bezierExpand];
/// the layout and the arithmetic are [`crate::convert::bezier_expand`]'s.
///
/// A spline of another type than 1 is [`spline_copy`]. Else [`spline_allocate`] (null ends the
/// routine), the closed and step words, type 1, [`spline_update_matrix`], the key count (`3 ×
/// keys`, less 2 when open, as 16 bits) and `SI_splineKeyAllocate` of it; an empty count ends
/// the routine. The loop reads the source's keys through their pointer, **do-while**: the
/// position is moved by integer moves, and the two control points are the chord's 0.33333 and
/// 0.66667 (x87, rounded to `float` where stored) when the following key's segment type is 2,
/// else the first key's `next` handle and the following key's `prev` handle, moved by
/// integer moves. The records `i + 1` and `i + 2` are written without testing the room (a
/// count that is not 1 modulo 3 after the 16-bit wrap writes past the used keys). An open source
/// of no keys makes the original read its keys through null; the read is the recorded fault
/// and the loop ends there.
pub fn spline_bezier_expand(m: &mut Mem, s: Ptr32<Spline>) -> Ptr32<Spline> {
    if m[s].kind != 1 {
        return spline_copy(m, s);
    }
    let out = spline_allocate(m);
    if out.is_null() {
        return out;
    }
    m[out].closed = m[s].closed;
    m[out].step = m[s].step;
    m[out].kind = 1;
    spline_update_matrix(m, out);
    let src_nb = nb(m, s);
    m[out].nb_keys = if m[s].closed != 0 { src_nb.wrapping_mul(3) } else { src_nb.wrapping_mul(3).wrapping_sub(2) } as i16;
    let block = spline_key_allocate(m, nb(m, out));
    m[out].keys = block;
    let count = nb(m, out);
    if count == 0 {
        return out;
    }
    let mut from = m[s].keys.addr();
    let mut dst = block.addr();
    let (mut i, mut idx) = (0i32, 0i32);
    loop {
        for w in 0..3u32 {
            let v = m.u32(from + 0x0c + 4 * w);
            m.set_u32(dst + 0x0c + 4 * w, v);
        }
        if m.fault().is_some() {
            break;
        }
        i += 1;
        dst = dst.wrapping_add(KEY);
        if i >= nb(m, out) {
            break;
        }
        let src_count = nb(m, s);
        let p = from;
        if idx == src_count - 1 {
            idx = 0;
            from = m[s].keys.addr();
        } else {
            idx += 1;
            from = from.wrapping_add(KEY);
        }
        if m.i16(from + 0x2a) == crate::SEGMENT_LINEAR {
            for (n, w) in [NEAR, FAR].into_iter().enumerate() {
                for j in 0..3u32 {
                    let (a, b) = (m.f32(p + 0x0c + 4 * j), m.f32(from + 0x0c + 4 * j));
                    let v = (x(a) + x(w) * (x(b) - a)).to_f32();
                    m.set_f32(dst + 0x0c + 4 * j, v);
                }
                if n == 0 {
                    dst = dst.wrapping_add(KEY);
                }
            }
            i += 2;
        } else {
            for w in 0..3u32 {
                let v = m.u32(p + 0x18 + 4 * w);
                m.set_u32(dst + 0x0c + 4 * w, v);
            }
            dst = dst.wrapping_add(KEY);
            for w in 0..3u32 {
                let v = m.u32(from + 4 * w);
                m.set_u32(dst + 0x0c + 4 * w, v);
            }
            i += 2;
        }
        dst = dst.wrapping_add(KEY);
        if i >= nb(m, out) {
            break;
        }
    }
    out
}
