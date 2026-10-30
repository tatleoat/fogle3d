// SPDX-License-Identifier: MIT
//! `SI_mergeTwoPatchs` of `patch.c` on the one memory image (finding 0020, section 20;
//! finding 0024): two patch models joined into one new patch along a picked border of
//! each.
//!
//! The routine works on copies of the two patches ([`patch_copy`]), brought into world
//! space with the models' global matrices, and builds the points and tags of a third copy.
//! Every allocation call of the original is made, with its size and in its order.

use si_math::algebra::hvector_multiply_matrix;
use si_math::types::Hvector;
use si_mem::{Access, Mem, Ptr32};
use si_types::geom::Patch;
use si_types::model::Model;
use si_x87::x;

use crate::patch_image::{patch_copy, patch_delete_row, patch_dispose, patch_update_surface, POINT};
use crate::spline_image::{Callee, Outside};

/// `SI_indexModifAllocate` (0x08176860): the table in which the modelling tools record
/// where each point of a model went; null when nobody wants one.
pub const SI_INDEX_MODIF_ALLOCATE: Callee = Callee { name: "SI_indexModifAllocate", addr: 0x0817_6860 };
/// `SI_indexModifPropagate` (0x08176988): hands such a table on (the model, the address
/// of the pointer, a 16-bit word).
pub const SI_INDEX_MODIF_PROPAGATE: Callee = Callee { name: "SI_indexModifPropagate", addr: 0x0817_6988 };
/// `SI_statusDisplayMessage` (0x083cca94): a text for the status line, and its kind.
pub const SI_STATUS_DISPLAY_MESSAGE: Callee = Callee { name: "SI_statusDisplayMessage", addr: 0x083c_ca94 };

/// The four message strings of the routine in `.data` (two texts, each twice).
pub const MSG_FIRST: [u32; 2] = [0x086d_6740, 0x086d_6780];
pub const MSG_SECOND: [u32; 2] = [0x086d_67c0, 0x086d_6800];

/// The address of the fault a port of this crate records where the original never
/// returns (a loop whose counter does not move). No memory is there.
pub const ENDLESS: u32 = 0xe9d1_0000;

/// A loop of the original that may not end: counts its turns and records a fault at
/// [`ENDLESS`] after more than any real patch needs.
struct Turns(u32);

impl Turns {
    fn more(&mut self, m: &Mem) -> bool {
        self.0 += 1;
        if self.0 > 1 << 22 {
            m.record_fault(ENDLESS, 0, Access::Read);
            return false;
        }
        m.fault().is_none()
    }
}

fn pt(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32) << 4)
}

fn tg(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32) << 1)
}

/// `new[i] = from[k]`, point and tag; `table[k] = i` when there is a table.
fn take(m: &mut Mem, np: u32, nt: u32, i: i32, from: Ptr32<Patch>, k: i32, table: u32) {
    m.copy(pt(np, i), pt(m[from].points.addr(), k), POINT);
    let t = m.u16(tg(m[from].tags.addr(), k));
    m.set_u16(tg(nt, i), t);
    if table != 0 {
        m.set_i32(table.wrapping_add((k as u32) << 2), i);
    }
}

/// `to = a + (b − a) × t` for x, y and z: the difference, the product with the float `t`
/// and the sum stay in the register; one rounding at the store.
fn mix(m: &mut Mem, to: u32, a: u32, b: u32, t: f32) {
    for c in [0u32, 4, 8] {
        let v = (x(m.f32(a + c)) + ((x(m.f32(b + c)) - m.f32(a + c)) * t)).to_f32();
        m.set_f32(to + c, v);
    }
}

/// Is the spline type a B-spline or a cardinal spline (`type − 2` at most 1, unsigned)?
fn overlapping(kind: i32) -> bool {
    (kind as u32).wrapping_sub(2) <= 1
}

/// Joins the patches of two models into a new patch [soft 0x08484364 SI_mergeTwoPatchs].
///
/// `models` points to the two model pointers. `a_dir` and `b_dir` say along which
/// direction each patch is joined (non-zero: its rows in U are appended; zero: its
/// columns in V); `index_a` and `index_b` are the picked display curves, from which the
/// routine decides which border of each patch is meant (`near`: the curve's number, less
/// the U-running set's count for a join in U, is below half the set's number of curves).
/// `delete` non-zero removes the picked border row of each patch first; `overlap` is the
/// number of rows the two patches share when the first one's type in the join direction
/// is B-spline or cardinal; `blend` (above 0.0001) moves the shared rows towards the
/// second patch.
///
/// In order:
///
/// 1. [`patch_copy`] of both definitions (`model + 0x48`); the two `near` flags;
///    `SI_indexModifAllocate(model, 0)` for each model.
/// 2. With `delete`: [`patch_delete_row`] on each copy, at point 0 when near, else at the
///    last point; on 0 `SI_statusDisplayMessage(text, 2)`, else
///    `SI_indexModifPropagate(model, &table, 0)`.
/// 3. `SI_indexModifAllocate(model, 1)` for each; the points of both copies through
///    their model's global matrix (`SI_hvectorMultiplyMatrix`); a third [`patch_copy`],
///    of the first copy: the result.
/// 4. The result's count in the join direction is the sum of the two (the second
///    patch's U count for `b_dir`, else its V count), less `overlap` for a B-spline or
///    cardinal first patch; new arrays `SI_calloc(n, 16)`, `SI_calloc(n, 2)`; the first
///    patch's points (reversed when `near`), then the second's, each with its table
///    entry; then the blend.
/// 5. A Bézier first patch gets two more rows between the parts (`SI_realloc` of both
///    arrays): the points a third (0.3333) and two thirds (0.6666) of the way.
/// 6. `SI_indexModifPropagate` for each table that is set; the result's old arrays freed
///    and the new ones stored; [`patch_dispose`] of both copies;
///    [`patch_update_surface`].
///
/// Neither copy is tested for null. Where a loop of the original cannot end (a patch
/// without points in a direction) a fault at [`ENDLESS`] is recorded.
#[allow(clippy::too_many_arguments)]
pub fn merge_two_patchs(m: &mut Mem, o: &mut dyn Outside, models: u32, a_dir: i16, index_a: i16, b_dir: i16, index_b: i16, delete: i16, overlap: i16, blend: f32) -> Ptr32<Patch> {
    // the four locals whose address is passed on: pb, pa, tb, ta
    let frame = m.local(0x10);
    let (l_pb, l_pa, l_tb, l_ta) = (frame, frame + 4, frame + 8, frame + 0xc);
    let model = |m: &Mem, which: u32| -> Ptr32<Model> { Ptr32::new(m.u32(models + 4 * which)) };
    let ov = overlap as i32;

    let ma = model(m, 0);
    let pa = patch_copy(m, m[ma].definition.cast());
    m.set_u32(l_pa, pa.addr());
    let mb = model(m, 1);
    let pb = patch_copy(m, m[mb].definition.cast());
    m.set_u32(l_pb, pb.addr());

    let near = |m: &Mem, p: Ptr32<Patch>, dir: i16, index: i16| -> bool {
        let (index, curves) = if dir != 0 { (index.wrapping_sub(m[p].surface0_dims[1] as i16), m[p].surface1_dims[1]) } else { (index, m[p].surface0_dims[1]) };
        (index as i32) < (curves >> 1) as i32
    };
    let near_a = near(m, pa, a_dir, index_a);
    let near_b = near(m, pb, b_dir, index_b);

    let t = o.call(m, &SI_INDEX_MODIF_ALLOCATE, &[model(m, 0).addr(), 0]);
    m.set_u32(l_ta, t);
    let t = o.call(m, &SI_INDEX_MODIF_ALLOCATE, &[model(m, 1).addr(), 0]);
    m.set_u32(l_tb, t);

    if delete != 0 {
        for (which, p, is_near, dir, table, text) in [(0u32, pa, near_a, a_dir, l_ta, MSG_FIRST), (1, pb, near_b, b_dir, l_tb, MSG_SECOND)] {
            let count = m[p].nb_u.wrapping_mul(m[p].nb_v);
            let at = if is_near { 0 } else { count.wrapping_sub(1) as i16 };
            let r = patch_delete_row(m, p, at, dir, m.u32(table));
            if r as i16 == 0 {
                o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[text[!is_near as usize], 2]);
            } else {
                o.call(m, &SI_INDEX_MODIF_PROPAGATE, &[model(m, which).addr(), table, 0]);
            }
        }
    }

    let t = o.call(m, &SI_INDEX_MODIF_ALLOCATE, &[model(m, 0).addr(), 1]);
    m.set_u32(l_ta, t);
    let t = o.call(m, &SI_INDEX_MODIF_ALLOCATE, &[model(m, 1).addr(), 1]);
    m.set_u32(l_tb, t);

    // both copies into world space
    let mut counts = [0i32; 2];
    for (which, p) in [(0u32, pa), (1, pb)] {
        let count = m[p].nb_u.wrapping_mul(m[p].nb_v) as i16;
        counts[which as usize] = count as i32;
        let pts = m[p].points.addr();
        let md = model(m, which);
        let mut i = 0;
        while i < count as i32 && m.fault().is_none() {
            let at: Ptr32<[f32; 4]> = Ptr32::new(pt(pts, i));
            let gmat = m[md].gmat;
            let v = hvector_multiply_matrix(&Hvector::from_array(m.read(at)), &gmat);
            m.write(at, v.to_array());
            i += 1;
        }
    }
    let [a_count, b_count] = counts;

    let pn = patch_copy(m, pa);
    let ta = |m: &Mem| m.u32(l_ta);
    let tb = |m: &Mem| m.u32(l_tb);
    let mut turns = Turns(0);
    let (mut np, mut nt);
    let along_u = a_dir != 0;

    if along_u {
        // ---- rows appended in U
        let sum = (if b_dir != 0 { m[pb].nb_u } else { m[pb].nb_v }).wrapping_add(m[pa].nb_u);
        m[pn].nb_u = sum;
        if overlapping(m[pa].u_type) {
            m[pn].nb_u = sum.wrapping_sub(overlap as u16);
        }
        let total = m[pn].nb_u.wrapping_mul(m[pn].nb_v) as i16 as i32;
        np = m.si_calloc(total as u32, POINT);
        nt = m.si_calloc(total as u32, 2);
        let mut i = 0i32;
        // the first patch: its rows, last first when the picked border is its first row
        if near_a {
            let mut k = a_count.wrapping_sub(m[pa].nb_v as i32);
            while i < a_count && turns.more(m) {
                let mut e = k;
                if m[pa].nb_v != 0 {
                    let mut j = 0;
                    loop {
                        take(m, np, nt, i, pa, e, ta(m));
                        i += 1;
                        j += 1;
                        e = e.wrapping_add(1);
                        if j >= m[pa].nb_v as i32 || m.fault().is_some() {
                            break;
                        }
                    }
                }
                k = k.wrapping_sub(m[pa].nb_v as i32);
            }
        } else {
            let mut k = 0;
            while i < a_count && m.fault().is_none() {
                take(m, np, nt, i, pa, k, ta(m));
                i += 1;
                k += 1;
            }
        }
        // the second patch
        i = a_count;
        if b_dir != 0 {
            if near_b {
                let mut k = if overlapping(m[pb].u_type) { ov.wrapping_mul(m[pb].nb_v as i32) } else { 0 };
                while i < total && m.fault().is_none() {
                    take(m, np, nt, i, pb, k, tb(m));
                    i += 1;
                    k = k.wrapping_add(1);
                }
            } else {
                let rows = if overlapping(m[pb].u_type) { ov.wrapping_add(1) } else { 1 };
                let mut k = b_count.wrapping_sub((m[pb].nb_v as i32).wrapping_mul(rows));
                while i < total && turns.more(m) {
                    let mut e = k;
                    if m[pb].nb_v != 0 {
                        let mut j = 0;
                        loop {
                            take(m, np, nt, i, pb, e, tb(m));
                            i += 1;
                            j += 1;
                            e = e.wrapping_add(1);
                            if j >= m[pb].nb_v as i32 || m.fault().is_some() {
                                break;
                            }
                        }
                    }
                    k = k.wrapping_sub(m[pb].nb_v as i32);
                }
            }
        } else {
            // the second patch's columns become rows
            let step = m[pb].nb_v as i16 as i32;
            let (mut k, by) = if near_b {
                (if overlapping(m[pb].v_type) { ov } else { 0 }, 1)
            } else {
                ((m[pb].nb_v as i32).wrapping_sub(if overlapping(m[pb].v_type) { ov } else { 0 }).wrapping_sub(1), -1)
            };
            while i < total && turns.more(m) {
                let mut e = k;
                if m[pb].nb_u != 0 {
                    let mut j = 0;
                    loop {
                        take(m, np, nt, i, pb, e, tb(m));
                        i += 1;
                        j += 1;
                        e = e.wrapping_add(step);
                        if j >= m[pb].nb_u as i32 || m.fault().is_some() {
                            break;
                        }
                    }
                }
                k = k.wrapping_add(by);
            }
        }
        // the blend of the shared rows
        if x(blend).gt(x(0.0001f64)) && overlap > 0 && overlapping(m[pa].u_type) {
            let mut i = a_count.wrapping_sub((m[pa].nb_v as i32).wrapping_mul(ov));
            if b_dir != 0 {
                if near_b {
                    let mut k = 0i32;
                    while i < a_count && m.fault().is_none() {
                        mix(m, pt(np, i), pt(np, i), pt(m[pb].points.addr(), k), blend);
                        i += 1;
                        k += 1;
                    }
                } else {
                    let mut k = b_count.wrapping_sub(m[pb].nb_v as i32);
                    while i < a_count && turns.more(m) {
                        let mut e = k;
                        if m[pa].nb_v != 0 {
                            let mut j = 0;
                            loop {
                                mix(m, pt(np, i), pt(np, i), pt(m[pb].points.addr(), e), blend);
                                e = e.wrapping_add(1);
                                j += 1;
                                i += 1;
                                if j >= m[pa].nb_v as i32 || m.fault().is_some() {
                                    break;
                                }
                            }
                        }
                        k = k.wrapping_sub(m[pa].nb_v as i32);
                    }
                }
            } else {
                let step = m[pb].nb_v as i16 as i32;
                let (mut k, by) = if near_b { (0, 1) } else { (m[pb].nb_v as i32 - 1, -1) };
                while i < a_count && turns.more(m) {
                    let mut e = k;
                    if m[pa].nb_v != 0 {
                        let mut j = 0;
                        loop {
                            mix(m, pt(np, i), pt(np, i), pt(m[pb].points.addr(), e), blend);
                            e = e.wrapping_add(step);
                            j += 1;
                            i += 1;
                            if j >= m[pa].nb_v as i32 || m.fault().is_some() {
                                break;
                            }
                        }
                    }
                    k = k.wrapping_add(by);
                }
            }
        }
        // a Bézier first patch: two rows of handles between the parts
        if m[pa].u_type == 1 {
            let va = m[pa].nb_v as i16 as i32;
            let old = (m[pn].nb_u as u32 * m[pn].nb_v as u32) as i32;
            m[pn].nb_u = m[pn].nb_u.wrapping_add(2);
            let size = m[pn].nb_u.wrapping_mul(m[pn].nb_v) as i16 as i32;
            np = m.si_realloc(np, (size as u32) << 4);
            nt = m.si_realloc(nt, (size as u32).wrapping_add(size as u32));
            let mut from_b = b_count.wrapping_sub(1);
            let mut i = old.wrapping_sub(1);
            let mut k = size.wrapping_sub(1);
            while k >= size.wrapping_sub((m[pa].nb_u as i32).wrapping_mul(va)) && m.fault().is_none() {
                m.copy(pt(np, k), pt(np, i), POINT);
                let t = m.u16(tg(nt, i));
                m.set_u16(tg(nt, k), t);
                let table = tb(m);
                if table != 0 {
                    m.set_i32(table.wrapping_add((from_b as u32) << 2), k);
                    from_b = from_b.wrapping_sub(1);
                }
                i = i.wrapping_sub(1);
                k = k.wrapping_sub(1);
            }
            let n0 = (m[pa].nb_u as i32).wrapping_mul(va);
            let (p, q) = (n0.wrapping_sub(va), n0.wrapping_add(va.wrapping_mul(2)));
            let mut e = 0;
            while e < va && m.fault().is_none() {
                for (c, to) in [(0.3333f64, n0.wrapping_add(e)), (0.6666, n0.wrapping_add(va).wrapping_add(e))] {
                    for d in [0u32, 4, 8] {
                        let (pv, qv) = (m.f32(pt(np, p.wrapping_add(e)) + d), m.f32(pt(np, q.wrapping_add(e)) + d));
                        let v = (x(pv) + (x(c) * (x(qv) - pv))).to_f32();
                        m.set_f32(pt(np, to) + d, v);
                    }
                }
                m.set_u16(tg(nt, n0.wrapping_add(e)), 0);
                m.set_u16(tg(nt, n0.wrapping_add(va).wrapping_add(e)), 0);
                e += 1;
            }
        }
    } else {
        // ---- columns appended in V
        let sum = (if b_dir != 0 { m[pb].nb_u } else { m[pb].nb_v }).wrapping_add(m[pa].nb_v);
        m[pn].nb_v = sum;
        if overlapping(m[pa].v_type) {
            m[pn].nb_v = sum.wrapping_sub(overlap as u16);
        }
        let total = m[pn].nb_u.wrapping_mul(m[pn].nb_v) as i16 as i32;
        np = m.si_calloc(total as u32, POINT);
        nt = m.si_calloc(total as u32, 2);
        // how many of the second patch's points a row gets
        let mut w = if b_dir != 0 { m[pb].nb_u } else { m[pb].nb_v };
        if overlapping(m[pa].v_type) {
            w = w.wrapping_sub(overlap as u16);
        }
        let per_row = w as i16 as i32;
        // the first patch: each row, backwards when the picked border is its first column
        let mut i = 0i32;
        if near_a {
            let mut k = m[pa].nb_v as i32 - 1;
            while k < a_count && turns.more(m) {
                let mut e = k;
                if m[pa].nb_v != 0 {
                    let mut j = 0;
                    loop {
                        take(m, np, nt, i, pa, e, ta(m));
                        i += 1;
                        j += 1;
                        e = e.wrapping_sub(1);
                        if j >= m[pa].nb_v as i32 || m.fault().is_some() {
                            break;
                        }
                    }
                }
                i = i.wrapping_add(per_row);
                k = k.wrapping_add(m[pa].nb_v as i32);
            }
        } else if i < a_count {
            let mut k = 0i32;
            loop {
                if m[pa].nb_v != 0 {
                    let mut j = 0;
                    loop {
                        take(m, np, nt, i, pa, k, ta(m));
                        i += 1;
                        k += 1;
                        j += 1;
                        if j >= m[pa].nb_v as i32 || m.fault().is_some() {
                            break;
                        }
                    }
                }
                i = i.wrapping_add(per_row);
                if k >= a_count || !turns.more(m) {
                    break;
                }
            }
        }
        // the second patch
        let mut i = m[pa].nb_v as i32;
        if b_dir != 0 {
            // its rows become columns
            let (mut k, by) = if near_b {
                (if overlapping(m[pb].u_type) { (m[pb].nb_v as i32).wrapping_mul(ov) } else { 0 }, 1)
            } else {
                let rows = if overlapping(m[pb].u_type) { ov.wrapping_add(1) } else { 1 };
                (b_count.wrapping_sub((m[pb].nb_v as i32).wrapping_mul(rows)), -1)
            };
            while i < total && turns.more(m) {
                let mut e = k;
                let mut left = per_row;
                while left > 0 && m.fault().is_none() {
                    take(m, np, nt, i, pb, e, tb(m));
                    i += 1;
                    e = e.wrapping_add(by * m[pb].nb_v as i32);
                    left -= 1;
                }
                k = k.wrapping_add(1);
                i = i.wrapping_add(m[pa].nb_v as i32);
            }
        } else if near_b {
            let mut skip = 0u16;
            let mut k = 0i32;
            if overlapping(m[pb].v_type) {
                skip = overlap as u16;
                k = ov;
            }
            while i < total && turns.more(m) {
                let mut left = per_row;
                while left > 0 && m.fault().is_none() {
                    take(m, np, nt, i, pb, k, tb(m));
                    i += 1;
                    k = k.wrapping_add(1);
                    left -= 1;
                }
                i = i.wrapping_add(m[pa].nb_v as i32);
                k = k.wrapping_add(skip as i16 as i32);
            }
        } else {
            let mut k = m[pb].nb_v as i32 - 1;
            let mut row = w;
            if overlapping(m[pb].v_type) {
                k = k.wrapping_sub(ov);
                row = m[pb].nb_v;
            }
            while i < total && turns.more(m) {
                let mut e = k;
                let mut left = per_row;
                while left > 0 && m.fault().is_none() {
                    take(m, np, nt, i, pb, e, tb(m));
                    i += 1;
                    e = e.wrapping_sub(1);
                    left -= 1;
                }
                i = i.wrapping_add(m[pa].nb_v as i32);
                k = k.wrapping_add(row as i16 as i32);
            }
        }
        // the blend of the shared columns
        if x(blend).gt(x(0.0001f64)) && overlap > 0 && overlapping(m[pa].v_type) {
            let mut w = m[pa].nb_v.wrapping_sub(overlap as u16);
            let mut ia = w as i16 as i32;
            let mut out = ia;
            let mut step = 1i32;
            let gap = m[pn].nb_v.wrapping_sub(overlap as u16) as i16 as i32;
            if near_a {
                ia = ov - 1;
                w = m[pa].nb_v.wrapping_add(overlap as u16);
                step = -1;
            }
            let per = w as i16 as i32;
            let mut k = if b_dir != 0 {
                if near_b {
                    0
                } else {
                    b_count.wrapping_sub(m[pb].nb_v as i32)
                }
            } else if near_b {
                0
            } else {
                m[pb].nb_v as i32 - 1
            };
            while ia < a_count && turns.more(m) {
                let mut e = k;
                let mut left = ov;
                while left > 0 && m.fault().is_none() {
                    mix(m, pt(np, out), pt(m[pa].points.addr(), ia), pt(m[pb].points.addr(), e), blend);
                    ia = ia.wrapping_add(step);
                    out = out.wrapping_add(1);
                    e = if b_dir != 0 {
                        if near_b {
                            e.wrapping_add(m[pb].nb_v as i32)
                        } else {
                            e.wrapping_sub(m[pb].nb_v as i32)
                        }
                    } else if near_b {
                        e.wrapping_add(1)
                    } else {
                        e.wrapping_sub(1)
                    };
                    left -= 1;
                }
                ia = ia.wrapping_add(per);
                k = if b_dir != 0 {
                    k.wrapping_add(1)
                } else if near_b {
                    // the cursor went on by the shared columns; the rest of the row
                    e.wrapping_add((m[pb].nb_v as i32).wrapping_sub(ov))
                } else {
                    k.wrapping_add(m[pb].nb_v as i32)
                };
                out = out.wrapping_add(gap);
            }
        }
        // a Bézier first patch: two columns of handles between the parts of every row
        if m[pa].v_type == 1 {
            let ua = m[pa].nb_u as i16 as i32;
            let old = (m[pn].nb_u as u32 * m[pn].nb_v as u32) as i32;
            let from_b = m[pn].nb_v.wrapping_sub(m[pa].nb_v) as i16 as i32;
            m[pn].nb_v = m[pn].nb_v.wrapping_add(2);
            let size = m[pn].nb_v.wrapping_mul(m[pn].nb_u) as i16 as i32;
            np = m.si_realloc(np, (size as u32) << 4);
            nt = m.si_realloc(nt, (size as u32).wrapping_add(size as u32));
            let mut k = size.wrapping_sub(1);
            let mut i = old.wrapping_sub(1);
            let shift = |m: &mut Mem, k: &mut i32, i: &mut i32| {
                m.copy(pt(np, *k), pt(np, *i), POINT);
                let t = m.u16(tg(nt, *i));
                m.set_u16(tg(nt, *k), t);
                *i = i.wrapping_sub(1);
                *k = k.wrapping_sub(1);
            };
            let mut left = from_b;
            while left > 0 && m.fault().is_none() {
                shift(m, &mut k, &mut i);
                left -= 1;
            }
            k = k.wrapping_sub(2);
            while i >= m[pa].nb_v as i32 && turns.more(m) {
                let mut e = 0;
                while e < m[pn].nb_v as i32 - 2 && m.fault().is_none() {
                    e += 1;
                    shift(m, &mut k, &mut i);
                }
                k = k.wrapping_sub(2);
            }
            // the index tables: a point of row r went on by 2 r
            for (table, p, last) in [(ta(m), pa, ua - 1), (tb(m), pb, ua)] {
                if table == 0 {
                    continue;
                }
                let mut i = if p == pa { m[pa].nb_v as i32 } else { 0 };
                let mut r = 1;
                while r <= last && m.fault().is_none() {
                    let mut e = 0;
                    while e < m[p].nb_v as i32 && m.fault().is_none() {
                        e += 1;
                        let at = table.wrapping_add((i as u32) << 2);
                        let v = m.i32(at).wrapping_add(r + r);
                        m.set_i32(at, v);
                        i += 1;
                    }
                    r += 1;
                }
            }
            let mut n = m[pa].nb_v as i32;
            let mut i = n - 1;
            let mut k = n + 2;
            let row = m[pn].nb_v as i16 as i32;
            let rest = m[pn].nb_v.wrapping_sub(1) as i16 as i32;
            while i < size && turns.more(m) {
                for c in [0.3333f64, 0.6666] {
                    for d in [0u32, 4, 8] {
                        let (pv, qv) = (m.f32(pt(np, i) + d), m.f32(pt(np, k) + d));
                        let v = if c == 0.3333 && d == 0 { ((x(c) * (x(qv) - pv)) + pv).to_f32() } else { (x(pv) + (x(c) * (x(qv) - pv))).to_f32() };
                        m.set_f32(pt(np, n) + d, v);
                    }
                    m.set_u16(tg(nt, n), 0);
                    if c == 0.3333 {
                        n = n.wrapping_add(1);
                    }
                }
                i = i.wrapping_add(row);
                k = k.wrapping_add(row);
                n = n.wrapping_add(rest);
            }
        }
    }

    for (which, table) in [(0u32, l_ta), (1, l_tb)] {
        if m.u32(table) != 0 {
            o.call(m, &SI_INDEX_MODIF_PROPAGATE, &[model(m, which).addr(), table, 0]);
        }
    }
    let old = m[pn].points.addr();
    if old != 0 {
        m.free(old);
        m[pn].points = Ptr32::NULL;
    }
    let old = m[pn].tags.addr();
    if old != 0 {
        m.free(old);
        m[pn].tags = Ptr32::NULL;
    }
    m[pn].points = Ptr32::new(np);
    m[pn].tags = Ptr32::new(nt);
    patch_dispose(m, l_pa);
    patch_dispose(m, l_pb);
    patch_update_surface(m, pn);
    m.release(0x10);
    pn
}
