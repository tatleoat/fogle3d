// SPDX-License-Identifier: MIT
//! Spline tools on the one memory image (finding 0020, section 16; finding 0024):
//! `SI_splineCut`, `SI_bezierSetMaximumConstraint` and `splineRefinement` of `spline.c`.

use si_math::algebra::{matrix_copy, points_colinear};
use si_math::types::Vector as MVector;
use si_mem::{Mem, Ptr32};
use si_types::geom::{Spline, SplineKey};
use si_x87::{x, Round, F80};

use crate::spline_image::{
    spline_add_point, spline_key_allocate, Outside, MODEL_CHAPTER, SI_ED_GET_ELEM_FROM_MODEL, SI_ELEMDICT_FIND_RELATED_ELEM, SI_ELEM_DICTS, SI_GBL_INDEX, SI_GBL_INDEX2, SI_GRAF_LISTS, SI_PROC_DATA,
    SI_WAVE_GET_ICON_MODEL,
};

const TINY: f64 = 1.0e-4;

fn v3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8))]
}

/// The distance of two points, left in the register: `√((dx² + dy²) + dz²)`.
fn dist(p: [f32; 3], q: [f32; 3]) -> F80 {
    let (dx, dy, dz) = (x(p[0]) - q[0], x(p[1]) - q[1], x(p[2]) - q[2]);
    (((dx * dx) + (dy * dy)) + (dz * dz)).sqrt()
}

/// Cuts an open spline in two at key `index` [soft 0x084911cc SI_splineCut]: `a` gets the
/// keys up to the cut, `b` the keys from it on; both are splines the caller made. `point`
/// is the picked place (the fourth component is not read): it decides which of the keys
/// `index − 1` and `index` the two parts share, the one it is nearer to (`index − 1` only
/// when the distance to key `index` is greater, both distances compared in the register).
///
/// - A closed source: nothing at all.
/// - Linear and Bézier, more than 2 keys: sharing key `index − 1`, `a` has `index` keys and
///   `b` `count − index + 1`; sharing key `index`, `index + 1` and `count − index`. Fewer
///   than 2 keys in either part is refused.
/// - B-spline and cardinal, more than 4 keys: one more key in each part (`index + 1` and
///   `count − index + 2`, or `index + 2` and `count − index + 1`); fewer than 4 in either
///   is refused.
/// - Refused (too few keys): `*want_b` and `*want_a` are cleared; the counts already
///   written to `a` and `b` stay.
/// - Otherwise each part gets a new key block (`SI_splineKeyAllocate`; an old one is not
///   freed) and the source's matrix; `a` the first keys (`SI_gblIndex[i] = i`), `b` the
///   last ones, copied from the end (`SI_gblIndex2[source index] = index in b`, **written
///   when `SI_gblIndex` is not null**: the test is of the other table). Then both parts
///   lose their phantom flag and take the source's closed word, type, tension and step.
///   A type outside 0..=3 gets only that last step.
///
/// Neither flag is read. The two keys' positions are read before anything is tested, so
/// an `index` of 0 reads the record before the block.
#[allow(clippy::too_many_arguments)]
pub fn spline_cut(m: &mut Mem, src: Ptr32<Spline>, a: Ptr32<Spline>, b: Ptr32<Spline>, want_a: u32, want_b: u32, point: [f32; 4], index: i32) {
    let k = m[src].keys.addr().wrapping_add((index as u32).wrapping_mul(0x30));
    let before = v3(m, k.wrapping_sub(0x24));
    let this = v3(m, k.wrapping_add(0x0c));
    if m[src].closed != 0 {
        return;
    }
    let p = [point[0], point[1], point[2]];
    let nearer_before = dist(p, this).gt(dist(p, before));
    let kind = m[src].kind;
    let nb = m[src].nb_keys as u16 as i32;
    let refuse = |m: &mut Mem| {
        m.set_i16(want_b, 0);
        m.set_i16(want_a, 0);
    };
    match kind {
        0 | 1 => {
            if nb <= 2 {
                return refuse(m);
            }
            m[a].nb_keys = (if nearer_before { index } else { index + 1 }) as i16;
            let nb = m[src].nb_keys as u16 as i32;
            m[b].nb_keys = (if nearer_before { nb - index + 1 } else { nb - index }) as i16;
            if m[a].nb_keys as u16 <= 1 || m[b].nb_keys as u16 <= 1 {
                return refuse(m);
            }
        }
        2 | 3 => {
            if nb <= 4 {
                return refuse(m);
            }
            m[a].nb_keys = (if nearer_before { index + 1 } else { index + 2 }) as i16;
            let nb = m[src].nb_keys as u16 as i32;
            m[b].nb_keys = (if nearer_before { nb - index + 2 } else { nb - index + 1 }) as i16;
            if m[a].nb_keys as u16 <= 3 || m[b].nb_keys as u16 <= 3 {
                return refuse(m);
            }
        }
        _ => {}
    }
    if (0..=3).contains(&kind) {
        let block = spline_key_allocate(m, m[a].nb_keys as u16 as i32);
        m[a].keys = block;
        m[a].matrix = matrix_copy(&m[src].matrix);
        let mut i = 0;
        if m[a].nb_keys != 0 {
            loop {
                let key = m.read(m[src].keys.add(i));
                m.write(m[a].keys.add(i), key);
                let table = m[SI_GBL_INDEX];
                if !table.is_null() {
                    m[table.add(i)] = i;
                }
                i += 1;
                if i >= m[a].nb_keys as u16 as i32 {
                    break;
                }
            }
        }
        let block = spline_key_allocate(m, m[b].nb_keys as u16 as i32);
        m[b].keys = block;
        m[b].matrix = matrix_copy(&m[src].matrix);
        let mut from = m[src].nb_keys as u16 as i32 - 1;
        let mut i = m[b].nb_keys as u16 as i32 - 1;
        while i >= 0 {
            let key = m.read(m[src].keys.add(from));
            m.write(m[b].keys.add(i), key);
            if !m[SI_GBL_INDEX].is_null() {
                let table = m[SI_GBL_INDEX2];
                m[table.add(from)] = i;
            }
            from -= 1;
            i -= 1;
        }
    }
    m[b].phantom = 0;
    m[a].phantom = 0;
    let closed = m[src].closed;
    m[b].closed = closed;
    m[a].closed = closed;
    let kind = m[src].kind;
    m[b].kind = kind;
    m[a].kind = kind;
    // through the register: fld, fst, fstp
    let tension = x(m[src].tension).to_f32();
    m[b].tension = tension;
    m[a].tension = tension;
    let step = m[src].step;
    m[b].step = step;
    m[a].step = step;
}

/// Gives every key of a Bézier spline the strongest constraint its handles satisfy, and
/// marks straight segments [soft 0x0848fc38 SI_bezierSetMaximumConstraint]. Other types:
/// nothing. Per key, in order:
///
/// - constraint 2 (mirrored), or 1 when `|2 × pos − prev − next|` is above 0.0001 in some
///   coordinate (formed as `((pos + pos) − prev) − next` in the register); a key that got
///   1 gets 4 (aligned) when `SI_pointsColinear(prev, pos, next)`;
/// - with `q` the key before (for the first key of a closed spline the last key; for the
///   first key of an open one this step is left out): segment type 2 (straight) when both
///   `|3 × q.next − 2 × q.pos − pos|` and `|3 × prev − q.pos − 2 × pos|` are within 0.0001
///   in every coordinate, that is when both handles of the segment lie at thirds of its
///   chord. A segment type that is 2 already stays; no other value is ever written.
///
/// Products with the double 3.0 and all sums stay in the register; the comparisons are
/// "above 0.0001" on the absolute value, so a NaN counts as within.
pub fn bezier_set_maximum_constraint(m: &mut Mem, s: Ptr32<Spline>) {
    if m[s].kind != 1 {
        return;
    }
    let mut k: Ptr32<SplineKey> = m[s].keys;
    let mut i = 0;
    if m[s].nb_keys == 0 {
        return;
    }
    let above = |v: F80| v.abs().gt(x(TINY));
    loop {
        let a = k.addr();
        let (prev, pos, next) = (v3(m, a), v3(m, a + 0x0c), v3(m, a + 0x18));
        m[k].constraint = 2;
        for c in 0..3 {
            if above(((x(pos[c]) + pos[c]) - prev[c]) - next[c]) {
                m[k].constraint = 1;
            }
        }
        if m[k].constraint == 1 {
            let v = |p: [f32; 3]| MVector::new(p[0], p[1], p[2]);
            if points_colinear(&v(prev), &v(pos), &v(next)) {
                m[k].constraint = 4;
            }
        }
        // the key the segment that ends here starts at
        let q = if i != 0 {
            Some(a.wrapping_sub(0x30))
        } else if m[s].closed != 0 {
            Some(m[s].keys.addr().wrapping_add((m[s].nb_keys as u16 as u32) * 0x30).wrapping_sub(0x30))
        } else {
            None
        };
        if let Some(q) = q {
            let (q_pos, q_next) = (v3(m, q.wrapping_add(0x0c)), v3(m, q.wrapping_add(0x18)));
            let mut straight = true;
            for c in 0..3 {
                if above(((x(q_next[c]) * 3.0f64) - (x(q_pos[c]) + q_pos[c])) - pos[c]) {
                    straight = false;
                }
            }
            for c in 0..3 {
                if above(((x(prev[c]) * 3.0f64) - q_pos[c]) - (x(pos[c]) + pos[c])) {
                    straight = false;
                }
            }
            if straight {
                m[k].segment = 2;
            }
        }
        i += 1;
        k = k.add(1);
        if i >= m[s].nb_keys as u16 as i32 {
            break;
        }
    }
}

/// `a + (b − a) × t` in the register.
fn lerp(a: f32, b: f32, t: F80) -> F80 {
    x(a) + ((x(b) - a) * t)
}

/// Inserts a key at the parameter `u` of a linear or Bézier spline
/// [soft 0x0849030c splineRefinement]; 1 when it did. No caller in the binary.
///
/// Refused (0): the types 2 and 3; `u` below 0 or above `keys + closed − 1`; a place
/// within 0.0001 of a key (`t = u − trunc(u)` below 0.0001 in absolute value, or `1 − t`
/// so). A NaN passes the first two tests; a type outside 0..=3 is refused after them.
///
/// With `k0` the key `trunc(u)` and `k1` the record after it (not wrapped on a closed
/// spline: the last segment reads the record past the keys in use):
///
/// - linear: the point `k0 + (k1 − k0) × t` (each coordinate rounded to float once) goes
///   to [`spline_add_point`] at `trunc(u) + 1`;
/// - Bézier with **constraint** 2 at `k1` (the constraint word, +0x28, not the segment
///   type): the same point; the new key's handles are set to it, its constraint to 1 and
///   its segment type to 2;
/// - Bézier otherwise: de Casteljau's construction. With A = `k0.pos → k0.next`,
///   B = `k0.next → k1.prev`, C = `k1.prev → k1.pos` at `t`, D = A → B, E = B → C and the
///   point P = D → E: A is rounded to float before use, B in x and y but **not in z**, C,
///   D and E are used unrounded, and what is stored is rounded to float. After the
///   insertion `k0.next = A`, the new key's handles are D and E, its constraint 3 and its
///   segment type 1, and C is written to the address `k1.prev` had **before** the
///   insertion: that is now the new key's `prev` handle (D is overwritten and `k1` keeps
///   its old handle), or, when the insertion moved the key block, the old block (and so
///   is A).
pub fn spline_refinement(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, u: f32) -> i32 {
    if (m[s].kind as u32).wrapping_sub(2) <= 1 {
        return 0;
    }
    let last = (m[s].nb_keys as u16 as i32).wrapping_add(m[s].closed as i32).wrapping_sub(1);
    if F80::ZERO.gt(x(u)) {
        return 0;
    }
    if F80::from_i32(last).lt(x(u)) {
        return 0;
    }
    let seg = x(u).to_i32(Round::Zero);
    let t = x(u) - F80::from_i32(seg);
    if x(TINY).gt(t.abs()) {
        return 0;
    }
    if (F80::ONE - t).abs().lt(x(TINY)) {
        return 0;
    }
    let k0 = m[s].keys.addr().wrapping_add((seg as u32).wrapping_mul(0x30));
    let k1 = k0.wrapping_add(0x30);
    let kind = m[s].kind;
    let point = m.local(12);
    let put = |m: &mut Mem, at: u32, v: [f32; 3]| {
        for c in 0..3 {
            m.set_f32(at + 4 * c, v[c as usize]);
        }
    };
    // the point on the chord; the keys are read only on the roads that use them
    let on_chord = |m: &Mem| {
        let (p0, p1) = (v3(m, k0.wrapping_add(0x0c)), v3(m, k1.wrapping_add(0x0c)));
        [lerp(p0[0], p1[0], t).to_f32(), lerp(p0[1], p1[1], t).to_f32(), lerp(p0[2], p1[2], t).to_f32()]
    };
    let done = if kind == 0 {
        let chord = on_chord(m);
        put(m, point, chord);
        spline_add_point(m, o, s, seg.wrapping_add(1), point);
        1
    } else if kind != 1 {
        0
    } else if m.i16(k1.wrapping_add(0x28)) == 2 {
        let chord = on_chord(m);
        put(m, point, chord);
        let n = spline_add_point(m, o, s, seg.wrapping_add(1), point).addr();
        put(m, n, chord);
        put(m, n + 0x18, chord);
        m.set_i16(n + 0x28, 1);
        m.set_i16(n + 0x2a, 2);
        1
    } else {
        let (p0, p1) = (v3(m, k0.wrapping_add(0x0c)), v3(m, k1.wrapping_add(0x0c)));
        let (h0, h1) = (v3(m, k0.wrapping_add(0x18)), v3(m, k1));
        let a = [lerp(p0[0], h0[0], t).to_f32(), lerp(p0[1], h0[1], t).to_f32(), lerp(p0[2], h0[2], t).to_f32()];
        let (bx, by, bz) = (lerp(h0[0], h1[0], t).to_f32(), lerp(h0[1], h1[1], t).to_f32(), lerp(h0[2], h1[2], t));
        let c = [lerp(h1[0], p1[0], t), lerp(h1[1], p1[1], t), lerp(h1[2], p1[2], t)];
        let dx = ((x(bx) - a[0]) * t) + a[0];
        let dy = ((x(by) - a[1]) * t) + a[1];
        let dz = ((bz - a[2]) * t) + a[2];
        let ex = ((c[0] - bx) * t) + bx;
        let ey = ((c[1] - by) * t) + by;
        let ez = ((c[2] - bz) * t) + bz;
        let p = [(((ex - dx) * t) + dx).to_f32(), (((ey - dy) * t) + dy).to_f32(), (((ez - dz) * t) + dz).to_f32()];
        put(m, point, p);
        let n = spline_add_point(m, o, s, seg.wrapping_add(1), point).addr();
        put(m, k0.wrapping_add(0x18), a);
        put(m, n, [dx.to_f32(), dy.to_f32(), dz.to_f32()]);
        put(m, n + 0x18, [ex.to_f32(), ey.to_f32(), ez.to_f32()]);
        put(m, k1, [c[0].to_f32(), c[1].to_f32(), c[2].to_f32()]);
        m.set_i16(n + 0x28, 3);
        m.set_i16(n + 0x2a, 1);
        1
    };
    m.release(12);
    done
}

/// Offsets the scene queries read in records this package does not lay out: the two
/// models at +0xa0 and +0xa4 of the record the third word of `SI_procData` points to; the
/// two models at +0x144 and +0x148 of the object of a graf node of kind 6, 8 or 0x13 (a
/// light, and two kinds this package cannot name); the path model at +4 of a spline
/// deformation record ([`crate::spline_image::FIT_PATH`]).
pub const PROC_MODEL_A: u32 = 0xa0;
pub const PROC_MODEL_B: u32 = 0xa4;
pub const OBJECT_MODEL_A: u32 = 0x144;
pub const OBJECT_MODEL_B: u32 = 0x148;

/// Whether a model is used as a path [soft 0x0848e794 SI_splineIsPath]: 1 when the model
/// (not null, of kind 6, a spline, or 9) is
///
/// - one of the two models at +0xa0 and +0xa4 of the record `SI_procData[2]` points to; or
/// - referred to by a node of the current object list: for a node of kind 2, 3 or 0xb (a
///   model's root or another of its nodes; for 0xb the model `SI_waveGetIconModel` gives
///   for the node's object), that model's path (+0x2c8) or the path model (+4) of either
///   of its spline deformation records (+0x60, +0x64); for a node of kind 6, 8 or 0x13,
///   +0x144 or +0x148 of its object.
///
/// 0 otherwise. Nodes of other kinds are passed over.
pub fn spline_is_path(m: &mut Mem, o: &mut dyn Outside, model: u32) -> i32 {
    use crate::spline_image::FIT_PATH;
    use si_types::model::Model;
    use si_types::scene::GrafNode;
    if model == 0 {
        return 0;
    }
    let me: Ptr32<Model> = Ptr32::new(model);
    if m[me].kind != 6 && m[me].kind != 9 {
        return 0;
    }
    let proc = m[SI_PROC_DATA].addr();
    let record = m.u32(proc.wrapping_add(8));
    if model == m.u32(record.wrapping_add(PROC_MODEL_A)) || model == m.u32(record.wrapping_add(PROC_MODEL_B)) {
        return 1;
    }
    let current = m.u32(proc.wrapping_add(4));
    let mut node: Ptr32<GrafNode> = Ptr32::new(m.u32(m[SI_GRAF_LISTS].wrapping_add(current.wrapping_mul(0x24)).wrapping_add(4)));
    while !node.is_null() {
        let kind = m[node].kind;
        match kind {
            2 | 3 | 0xb => {
                let object: Ptr32<Model> = if kind == 0xb { Ptr32::new(o.call(m, &SI_WAVE_GET_ICON_MODEL, &[m[node].data.addr()])) } else { m[node].data.cast() };
                if model == m[object].path.addr() {
                    return 1;
                }
                let fit = m[object].spline_fit[0].addr();
                if fit != 0 && model == m.u32(fit + FIT_PATH) {
                    return 1;
                }
                let fit = m[object].spline_fit[1].addr();
                if fit != 0 && model == m.u32(fit + FIT_PATH) {
                    return 1;
                }
            }
            6 | 8 | 0x13 => {
                let object = m[node].data.addr();
                if model == m.u32(object.wrapping_add(OBJECT_MODEL_A)) || model == m.u32(object.wrapping_add(OBJECT_MODEL_B)) {
                    return 1;
                }
            }
            _ => {}
        }
        node = m[node].next;
    }
    0
}

/// Whether a model's element stands in a relation 0xfa that does not lead back to it
/// [soft 0x0848e888 SI_splineIsInExtRel]: with `e = SI_edGetElemFromModel(model, &n)` and
/// `d` the current element dictionary (`SI_elemDicts + SI_procData[0] × 0x70`):
/// `r = SI_elemdictFindRelatedElem(d, e, 0, 2, 0xfa, chapter, &n, &n)` and then
/// `SI_elemdictFindRelatedElem(d, r, 1, 2, 0xfa, chapter, &n, &n)`, the chapter being the
/// models'. 1 when both exist and the second is not `e`; 0 when the model has no element,
/// when there is no such relation, or when the way back ends at `e`.
pub fn spline_is_in_ext_rel(m: &mut Mem, o: &mut dyn Outside, model: u32) -> i32 {
    let local = m.local(4);
    let mut answer = 0;
    let e = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[model, local]);
    if e != 0 {
        let dict = |m: &Mem| {
            let index = m[m[SI_PROC_DATA]][0] as u32;
            (index << 3).wrapping_sub(index).wrapping_shl(4).wrapping_add(m[SI_ELEM_DICTS])
        };
        let d = dict(m);
        let chapter = m[MODEL_CHAPTER] as u32;
        let r = o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM, &[d, e, 0, 2, 0xfa, chapter, local, local]);
        if r != 0 {
            let d = dict(m);
            let chapter = m[MODEL_CHAPTER] as u32;
            let back = o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM, &[d, r, 1, 2, 0xfa, chapter, local, local]);
            answer = (back != e) as i32;
        }
    }
    m.release(4);
    answer
}
