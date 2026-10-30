// SPDX-License-Identifier: MIT
//! `SI_intersectionPerform` of `spline.c` on the one memory image (finding 0020, section
//! 21; finding 0024), and the image forms of what it calls: the copy, the transfer, the
//! disposal and the subdivision of a spline.
//!
//! Two leaf computations are reached through the value forms, which are cited and compared
//! there and allocate nothing: the point at a parameter ([`spline_value`]) and the cubic
//! solver ([`solve_cubic`]). Everything else is written in place.

use si_math::algebra::matrix_copy;
use si_mem::{Mem, Ptr32};
use si_types::geom::{Spline, SplineKey};
use si_x87::{fmov, x, F80};

use crate::spline_image::{spline_add_point, spline_allocate, spline_key_allocate, spline_touch, Outside};
use crate::Curve;

/// The spline at `s` as the value forms hold one: the record and its key block (the room
/// the original allocates: a multiple of 16 records, and one block more, for the routines
/// that read a record past the keys in use).
pub(crate) fn load_curve(m: &Mem, s: Ptr32<Spline>) -> Curve {
    let rec = m.read(s);
    let nb = rec.nb_keys as u16 as usize;
    let room = if rec.keys.is_null() { 0 } else { nb.div_ceil(16) * 16 + 16 };
    Curve { rec, name: None, keys: (0..room as i32).map(|i| m.read(rec.keys.add(i))).collect(), lengths: Vec::new() }
}

/// `SI_splineNbSegment` (0x0848c8dc; cited at [`crate::eval::nb_segment`]): it reads the
/// record only.
pub fn spline_nb_segment(m: &Mem, s: Ptr32<Spline>) -> i32 {
    crate::eval::nb_segment(&Curve { rec: m.read(s), name: None, keys: Vec::new(), lengths: Vec::new() })
}

/// `SI_splineValue` (0x0848b1e4): the point at a parameter, stored at `out`; through the
/// value form [`crate::eval::value`] (cited and compared there). It allocates nothing and
/// writes the three floats only.
// adapter: not in place
pub fn spline_value(m: &mut Mem, s: Ptr32<Spline>, u: f32, out: u32) -> i32 {
    let (p, r) = crate::eval::value(&load_curve(m, s), u);
    m.set_f32(out, p.x);
    m.set_f32(out + 4, p.y);
    m.set_f32(out + 8, p.z);
    r
}

/// `SI_solveCubic` (0x08491f70) on two arrays of the image: through the value form
/// [`crate::tools::solve_cubic`] (cited and compared there). The first coefficient is
/// replaced by its difference from the target, the roots found are stored. Where the
/// original answers what a register happened to hold, the answer here is no root.
// adapter: not in place
pub fn solve_cubic(m: &mut Mem, coef: u32, roots: u32, target: f32) -> i16 {
    let mut c = [m.f32(coef), m.f32(coef + 4), m.f32(coef + 8), m.f32(coef + 12)];
    let r = crate::tools::solve_cubic(&mut c, target);
    m.set_f32(coef, c[0]);
    let Some((found, n)) = r else {
        return 0;
    };
    for i in 0..(n.max(0) as usize).min(3) {
        m.set_f32(roots + 4 * i as u32, found[i]);
    }
    n
}

/// Copies the content of `from` into `to` [soft 0x0848aa9c SI_splineTransfer]: the closed
/// and phantom words, the key count, a new key block
/// ([`spline_key_allocate`] of the count; `to`'s old block is not freed) with the keys in
/// use copied whole, then step, type, tension (through the unit) and the matrix
/// (`SI_matrixCopy`).
pub fn spline_transfer(m: &mut Mem, from: Ptr32<Spline>, to: Ptr32<Spline>) {
    m[to].closed = m[from].closed;
    m[to].phantom = m[from].phantom;
    m[to].nb_keys = m[from].nb_keys;
    let block = spline_key_allocate(m, m[from].nb_keys as u16 as i32);
    m[to].keys = block;
    let mut i = 0;
    while i < m[from].nb_keys as u16 as i32 && m.fault().is_none() {
        let key: SplineKey = m.read(m[from].keys.add(i));
        m.write(m[to].keys.add(i), key);
        i += 1;
    }
    m[to].step = m[from].step;
    m[to].kind = m[from].kind;
    m[to].tension = fmov(m[from].tension);
    let matrix = matrix_copy(&m[from].matrix);
    m[to].matrix = matrix;
}

/// A copy of a spline [soft 0x0848aa74 SI_splineCopy]: [`spline_allocate`] (null ends the
/// routine), then [`spline_transfer`].
pub fn spline_copy(m: &mut Mem, from: Ptr32<Spline>) -> Ptr32<Spline> {
    let to = spline_allocate(m);
    if !to.is_null() {
        spline_transfer(m, from, to);
    }
    to
}

/// Frees a spline [soft 0x0848ab48 SI_splineDispose]: `*pp` not null: the key block, the
/// length table and the name, each freed and cleared when set, then the record; `*pp`
/// cleared.
pub fn spline_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    for off in [0x14u32, 0x6c, 0] {
        let a = m.u32(m.u32(pp).wrapping_add(off));
        if a != 0 {
            m.free(a);
            let s = m.u32(pp);
            m.set_u32(s.wrapping_add(off), 0);
        }
    }
    let s = m.u32(pp);
    if s != 0 {
        m.free(s);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, 0);
}

fn vec3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

fn put3(m: &mut Mem, a: u32, v: [f32; 3]) {
    for (k, c) in v.iter().enumerate() {
        m.set_u32(a + 4 * k as u32, c.to_bits());
    }
}

/// Splits segment `seg` at `t` [soft 0x08492294 splineSubdivision]; 0 when refused: the
/// types 2 and 3, `t` below 0.0001 or above 0.9999, a type outside 0..=3.
///
/// **Linear**: the point at `float(seg + t)` ([`spline_value`]) is added as key `seg + 1`
/// ([`spline_add_point`]). **Bézier**: de Casteljau's construction on the segment's four
/// control points, in the original's order of operations
/// ([`crate::convert::subdivision`] has it in words); the new point is added, and then
/// the first key's `next` handle, the new key's two handles and the following key's
/// `prev` handle are written **through pointers taken before the point was added**: when
/// the key block moved, the first and the last of those land in the old block. All three
/// keys get constraint 1. Both ways end with [`spline_touch`].
pub fn spline_subdivision(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, seg: i32, t: f32) -> i32 {
    if (m[s].kind as u32).wrapping_sub(2) <= 1 || x(t).lt(x(1e-4f64)) || x(t).gt(x(0.9999f64)) {
        return 0;
    }
    let keys = m[s].keys.addr();
    let first = keys.wrapping_add((seg as u32).wrapping_mul(0x30));
    let mut second = if seg == m[s].nb_keys as u16 as i32 - 1 { keys } else { first.wrapping_add(0x30) };
    let w = (F80::ONE - t).to_f32();
    match m[s].kind {
        1 => {
            let (p0, p1, p2, p3) = (vec3(m, first + 0xc), vec3(m, first + 0x18), vec3(m, second), vec3(m, second + 0xc));
            let lerp = |a: f32, b: f32| x(t) * b + x(w) * a;
            let a = [lerp(p0[0], p1[0]), lerp(p0[1], p1[1]), lerp(p0[2], p1[2])];
            let d = [lerp(p2[0], p3[0]), lerp(p2[1], p3[1]), lerp(p2[2], p3[2])];
            let bx = lerp(p1[0], p2[0]);
            let by = (x(t) * p2[1] + (x(w) * p1[1]).to_f32()).to_f32();
            let bz = (x(t) * p2[2] + (x(w) * p1[2]).to_f32()).to_f32();
            let cx = (x(t) * bx + (a[0] * x(w)).to_f32()).to_f32();
            let cy = (x(t) * by + (a[1] * x(w)).to_f32()).to_f32();
            let cz = (x(t) * bz + (a[2] * x(w)).to_f32()).to_f32();
            let ex = d[0] * x(t) + (bx * x(w)).to_f32();
            let ey = x(t) * d[1] + x(w) * by;
            let ez = x(t) * d[2] + x(w) * bz;
            let f = [(ex * x(t) + x(w) * cx).to_f32(), (ey * x(t) + x(w) * cy).to_f32(), (x(w) * cz + ez * x(t)).to_f32()];
            let round = |p: [F80; 3]| [p[0].to_f32(), p[1].to_f32(), p[2].to_f32()];
            let (a, d, e) = (round(a), round(d), round([ex, ey, ez]));
            // the new point by address: a local of the original
            let local = m.local(12);
            put3(m, local, f);
            let new = spline_add_point(m, o, s, seg + 1, local).addr();
            m.release(12);
            if seg != m[s].nb_keys as u16 as i32 - 2 {
                second = second.wrapping_add(0x30);
            }
            put3(m, first + 0x18, a);
            m.set_u16(first + 0x28, 1);
            put3(m, new, [cx, cy, cz]);
            put3(m, new + 0x18, e);
            m.set_u16(new + 0x28, 1);
            put3(m, second, d);
            m.set_u16(second + 0x28, 1);
        }
        0 => {
            let local = m.local(12);
            spline_value(m, s, (F80::from_i32(seg) + t).to_f32(), local);
            spline_add_point(m, o, s, seg + 1, local);
            m.release(12);
        }
        _ => return 0,
    }
    spline_touch(m, o, s);
    1
}

/// C's `%W.Pf` for a `double` as the GNU C library prints it (the same few lines as
/// `si-math`'s, which are private there).
fn printf_f(v: f64, width: usize, precision: usize) -> String {
    let body = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{v:.precision$}")
    };
    format!("{body:>width$}")
}

/// The size of an entry of the table of intersections: two 16-bit words the routine
/// clears, the address of the text, and twelve bytes it leaves as `calloc` made them.
pub const ENTRY: u32 = 0x14;

/// The power-basis coefficients (constant first) of a Bézier segment's coordinate:
/// `c1 = float(float(−3 p0) + float(3 p1))`, `c2 = float(float(float(3 p0) +
/// float(−6 p1)) + float(3 p2))`, `c3 = float((float(−3 p2) + (float(3 p1) − p0)) + p3)`;
/// for y the difference `float(3 p1) − p0` is rounded to float first.
fn bezier_coef(p: [f32; 4], y: bool) -> [f32; 4] {
    let three_p1 = (x(3.0f64) * p[1]).to_f32();
    let c1 = (x((x(p[0]) * -3.0f64).to_f32()) + three_p1).to_f32();
    let low = (x((x(p[0]) * 3.0f64).to_f32()) + (x(p[1]) * -6.0f64).to_f32()).to_f32();
    let c2 = (x((x(p[2]) * 3.0f64).to_f32()) + low).to_f32();
    let d = x(three_p1) - p[0];
    let d = if y { x(d.to_f32()) } else { d };
    let c3 = ((x((x(-3.0f64) * p[2]).to_f32()) + d) + p[3]).to_f32();
    [fmov(p[0]), c1, c2, c3]
}

/// The same of a B-spline segment with the four control values k:
/// `c0 = float(⅙ float((float(4 k1) + k0) + k2))`, `c1 = float((k2 − k0) × ½)`,
/// `c2 = float(((float(3 k0) + float(−6 k1)) + float(3 k2)) × ⅙)`,
/// `c3 = float((((float(3 k1) − k0) + float(−3 k2)) + k3) × ⅙)`; for z the sum
/// `float(4 k1) + k0` is rounded to float before `k2` is added. ⅙ is the double.
fn bspline_coef(k: [f32; 4], z: bool) -> [f32; 4] {
    let sixth = x(0.16666666666666666f64);
    let c3 = ((((x((x(3.0f64) * k[1]).to_f32()) - k[0]) + (x(k[2]) * -3.0f64).to_f32()) + k[3]) * sixth).to_f32();
    let c2 = (((x((x(3.0f64) * k[0]).to_f32()) + (x(k[1]) * -6.0f64).to_f32()) + (x(k[2]) * 3.0f64).to_f32()) * sixth).to_f32();
    let c1 = ((x(k[2]) - k[0]) * 0.5f64).to_f32();
    let s = x((x(k[1]) * 4.0f64).to_f32()) + k[0];
    let s = if z { x(s.to_f32()) } else { s };
    let c0 = (sixth * (s + k[2]).to_f32()).to_f32();
    [c0, c1, c2, c3]
}

/// The same of a cardinal segment, **with the tension ½ whatever the spline's is**:
/// `c0 = k1`, `c1 = float((k2 − k0) × ½)` (for y and z the difference is rounded first),
/// `c2 = float((((float(k0 + k0) + float(−5 k1)) + float(4 k2)) − k3) × ½)`,
/// `c3 = float((((float(3 k1) − k0) + float(−3 k2)) + k3) × ½)`.
fn cardinal_coef(k: [f32; 4], x_axis: bool) -> [f32; 4] {
    let c3 = ((((x((x(k[1]) * 3.0f64).to_f32()) - k[0]) + (x(k[2]) * -3.0f64).to_f32()) + k[3]) * 0.5f64).to_f32();
    let c2 = ((((x((x(k[0]) + k[0]).to_f32()) + (x(k[1]) * -5.0f64).to_f32()) + (x(k[2]) * 4.0f64).to_f32()) - k[3]) * 0.5f64).to_f32();
    let d = x(k[2]) - k[0];
    let d = if x_axis { d } else { x(d.to_f32()) };
    let c1 = (d * 0.5f64).to_f32();
    [fmov(k[1]), c1, c2, c3]
}

/// Where a spline crosses a plane of constant x, y or z
/// [soft 0x084926b8 SI_intersectionPerform].
///
/// `axis` 0 is the plane `z = c`, 1 the plane `y = b`, 2 the plane `x = a`; any other
/// axis solves nothing (the number of roots is then a local nobody set: none here).
/// `mode` zero lists the intersections; non-zero cuts the spline at them.
///
/// - `SI_splineNbSegment`; `SI_malloc(8)` for the result record (a 16-bit count and the
///   address of the table); with `mode`, [`spline_copy`] of the spline.
/// - The table: `SI_calloc(segments, 0x14)` for a linear spline and
///   `SI_calloc(3 × segments, 0x14)` for a Bézier one, both only without `mode`; for a
///   B-spline and a cardinal spline `SI_calloc(3 × segments, 0x14)` always.
/// - Per segment, the cubic in the plane's coordinate from the segment's control values
///   (linear: `p0 + (p1 − p0) t`; the others: `bezier_coef`, `bspline_coef`,
///   `cardinal_coef` in this file) goes to [`solve_cubic`] with the plane's value. A
///   B-spline or cardinal segment's four keys are taken in sequence from key `seg`,
///   starting again at key 0 after the last key; **the count that notices the last key is
///   set back to `seg` each time, so the last segments of a closed spline repeat key 0**
///   (reproduced).
/// - Each root `t` with `−1e-6 < t < 1.000001`: the point at `float(seg + t)`
///   ([`spline_value`]). Listing (always for the types 2 and 3): the entry's two words
///   cleared, `SI_malloc(0x40)` for the text and
///   `sprintf(text, "%3hd)     %7.5f  %7.5f  %7.5f     %7.5f", seg, x, y, z, t)`.
///   Cutting (types 0 and 1 with `mode`): [`spline_subdivision`] of the copy at segment
///   `seg + cuts so far`, at `float(t / last)`, `last` the last root cut at in this
///   segment (1.0 at first).
/// - The end. With `mode`: no cut made: 0 (the copy and the record stay allocated);
///   else [`spline_transfer`] of the copy into the spline, [`spline_dispose`] of the copy,
///   and the record's address. Without: the count is stored; the record's address when
///   it is not 0, else the record is freed and the answer is 0.
#[allow(clippy::too_many_arguments)]
pub fn intersection_perform(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, axis: i16, a: f32, b: f32, c: f32, mode: i16) -> u32 {
    let mut cuts = 0i32;
    let mut found = 0i32;
    let segments = spline_nb_segment(m, s);
    let info = m.si_malloc(8);
    // the copy's address is a local whose address goes to SI_splineDispose
    let l_copy = m.local(4);
    if mode != 0 {
        let copy = spline_copy(m, s);
        m.set_u32(l_copy, copy.addr());
    }
    let kind = m[s].kind;
    if (0..=3).contains(&kind) {
        if kind >= 2 || mode == 0 {
            let table = m.si_calloc((if kind == 0 { segments } else { segments.wrapping_mul(3) }) as u32, ENTRY);
            m.set_u32(info + 4, table);
        }
        // the coordinate the plane fixes, and the plane's value
        let (coord, target) = match axis {
            0 => (Some(2usize), c),
            1 => (Some(1), b),
            2 => (Some(0), a),
            _ => (None, 0.0),
        };
        let frame = m.local(0x28);
        let (l_coef, l_roots, l_point) = (frame, frame + 0x10, frame + 0x1c);
        let mut seg = 0i32;
        while seg < segments && m.fault().is_none() {
            for k in 0..3 {
                m.set_f32(l_roots + 4 * k, 0.0);
            }
            let nb = m[s].nb_keys as u16 as i32;
            let keys = m[s].keys.addr();
            let at = |i: i32| keys.wrapping_add((i as u32).wrapping_mul(0x30));
            let mut roots = 0i32;
            if let Some(d) = coord {
                let v = |m: &Mem, addr: u32| m.f32(addr + 4 * d as u32);
                let coef = match kind {
                    0 => {
                        let p0 = v(m, at(seg) + 0xc);
                        let p1 = v(m, if seg == nb - 1 { keys } else { at(seg) + 0x30 } + 0xc);
                        [fmov(p0), (x(p1) - p0).to_f32(), 0.0, 0.0]
                    }
                    1 => {
                        let next = if seg == nb - 1 { keys } else { at(seg) + 0x30 };
                        bezier_coef([v(m, at(seg) + 0xc), v(m, at(seg) + 0x18), v(m, next), v(m, next + 0xc)], d == 1)
                    }
                    _ => {
                        // four keys in sequence; the count is set back to `seg` at a wrap
                        let mut count = seg;
                        let mut key = if count == nb {
                            keys
                        } else {
                            count += 1;
                            at(seg)
                        };
                        let mut k = [0f32; 4];
                        k[0] = v(m, key + 0xc);
                        for (i, slot) in k.iter_mut().enumerate().skip(1) {
                            if count == nb {
                                key = keys;
                                count = seg;
                            } else {
                                key = key.wrapping_add(0x30);
                                if i < 3 {
                                    count += 1;
                                }
                            }
                            *slot = v(m, key + 0xc);
                        }
                        if kind == 2 {
                            bspline_coef(k, d == 2)
                        } else {
                            cardinal_coef(k, d == 0)
                        }
                    }
                };
                for (i, w) in coef.iter().enumerate() {
                    m.set_f32(l_coef + 4 * i as u32, *w);
                }
                roots = solve_cubic(m, l_coef, l_roots, target) as i32;
            }
            let mut last = 1.0f32;
            let mut here = 0i32;
            let mut entry = (found as u32).wrapping_mul(ENTRY);
            let mut i = 0;
            while i < roots && m.fault().is_none() {
                let t = m.f32(l_roots.wrapping_add((i as u32) << 2));
                i += 1;
                if !(x(t).lt(x(1.000001f64)) && x(t).gt(x(-1e-6f64))) {
                    continue;
                }
                spline_value(m, s, (F80::from_i32(seg) + t).to_f32(), l_point);
                if kind < 2 && mode != 0 {
                    let copy = Ptr32::new(m.u32(l_copy));
                    if spline_subdivision(m, o, copy, seg.wrapping_add(cuts), (x(t) / last).to_f32()) as i16 != 0 {
                        last = t;
                        here += 1;
                    }
                } else {
                    let table = m.u32(info + 4);
                    m.set_u16(table.wrapping_add(entry), 0);
                    let table = m.u32(info + 4);
                    m.set_u16(table.wrapping_add(entry).wrapping_add(2), 0);
                    let text = m.si_malloc(0x40);
                    let table = m.u32(info + 4);
                    m.set_u32(table.wrapping_add(entry).wrapping_add(4), text);
                    let p = vec3(m, l_point);
                    let line = format!("{:>3})     {}  {}  {}     {}", seg as i16, printf_f(p[0] as f64, 7, 5), printf_f(p[1] as f64, 7, 5), printf_f(p[2] as f64, 7, 5), printf_f(t as f64, 7, 5));
                    let to = m.u32(m.u32(info + 4).wrapping_add(entry).wrapping_add(4));
                    m.put_cstr(to, line.as_bytes());
                    entry = entry.wrapping_add(ENTRY);
                    found += 1;
                }
            }
            cuts = cuts.wrapping_add(here);
            seg += 1;
        }
        m.release(0x28);
    }
    let result = if mode != 0 {
        if cuts == 0 {
            0
        } else {
            spline_transfer(m, Ptr32::new(m.u32(l_copy)), s);
            spline_dispose(m, l_copy);
            info
        }
    } else {
        m.set_u16(info, found as u16);
        if found as i16 != 0 {
            info
        } else {
            if info != 0 {
                m.free(info);
            }
            0
        }
    };
    m.release(4);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spline_image::{self, NoOutside};

    /// A linear spline of three keys along x, with an empty object list.
    fn line(m: &mut Mem) -> Ptr32<Spline> {
        spline_image::init(m);
        let lists = m.calloc(1, 0x24);
        m[spline_image::SI_GRAF_LISTS] = lists;
        let s = spline_allocate(m);
        let keys = spline_key_allocate(m, 3);
        m[s].keys = keys;
        m[s].nb_keys = 3;
        for i in 0..3 {
            m[keys.add(i)].pos.x = i as f32;
        }
        s
    }

    #[test]
    fn a_plane_across_a_line_is_listed_with_its_text() {
        let mut m = Mem::new();
        let s = line(&mut m);
        // the plane x = 1.5 (axis 2): one crossing, on the second segment
        let info = intersection_perform(&mut m, &mut NoOutside, s, 2, 1.5, 0.0, 0.0, 0);
        assert_eq!(m.u16(info), 1);
        let text = m.u32(m.u32(info + 4) + 4);
        assert_eq!(m.cstr(text), b"  1)     1.50000  0.00000  0.00000     0.50000");
        assert!(m.fault().is_none());
    }

    #[test]
    fn cutting_adds_a_key_at_the_crossing() {
        let mut m = Mem::new();
        let s = line(&mut m);
        assert_ne!(intersection_perform(&mut m, &mut NoOutside, s, 2, 0.25, 0.0, 0.0, 1), 0);
        assert_eq!(m[s].nb_keys, 4);
        assert_eq!(m[m[s].keys.add(1)].pos.x, 0.25);
        // nothing to cut: no result
        assert_eq!(intersection_perform(&mut m, &mut NoOutside, s, 2, 9.0, 0.0, 0.0, 1), 0);
        assert!(m.fault().is_none());
    }
}
