// SPDX-License-Identifier: MIT
//! Patch editing on the one memory image (finding 0020, section 15; finding 0024):
//! `SI_patchAddRow`, and what it needs of the display update.
//!
//! The patch is `si-types`' `SI_Patch` at its address; its points are 16 bytes each (x, y,
//! z and a fourth float), its tags one 16-bit word a point, both indexed `i × nb_v + j`
//! (U is the major direction). Every `SI_calloc`, `SI_realloc` and `free` of the original
//! is made, with its size and in its order.
//!
//! One routine is reached through an adapter: the display curves
//! (`SI_patchComputeSurface`) are computed by the value form
//! ([`crate::patch_grid::compute_surface`]) on a copy of the patch, see
//! [`patch_compute_surface`].

use si_mem::{Access, Mem, Ptr32};
use si_types::geom::{Patch, Spline, Vector};
use si_math::algebra::matrix_transpose;
use si_x87::{fmov, x};

use crate::spline_image::{spline_allocate, spline_key_allocate, spline_update_matrix};

use crate::eval::nb_segment_step;
use crate::patch::Surface;

/// The address of the fault an adapter of this crate records where it cannot do what the
/// original does (memory outside the arrays a value form holds). No memory is there.
pub const ADAPTER_LIMIT: u32 = 0xada0_0000;

/// The size of a control point: x, y, z and a fourth float.
pub const POINT: u32 = 0x10;

/// The signed division of the original (`cdq`, `idiv`): quotient towards zero and the
/// remainder with the dividend's sign. A divisor of 0 raises the processor's divide error
/// in the original; here it is a recorded fault and the answer is zeros.
pub(crate) fn idiv(m: &Mem, a: i32, b: i32) -> (i32, i32) {
    if b == 0 || (a == i32::MIN && b == -1) {
        m.record_fault(0, 0, Access::Read);
        return (0, 0);
    }
    (a.wrapping_div(b), a.wrapping_rem(b))
}

fn vectors(m: &Mem, at: u32, n: usize) -> Vec<Vector> {
    (0..n as u32).map(|i| Vector { x: m.f32(at + 12 * i), y: m.f32(at + 12 * i + 4), z: m.f32(at + 12 * i + 8) }).collect()
}

fn words(m: &Mem, at: u32, n: usize) -> Vec<i16> {
    (0..n as u32).map(|i| m.i16(at + 2 * i)).collect()
}

/// The patch at `p` with its arrays, as the value forms hold one. The shading grid is
/// left out (no routine here reads it).
pub(crate) fn load_surface(m: &Mem, p: Ptr32<Patch>) -> Surface {
    let rec = m.read(p);
    let n = rec.nb_u as usize * rec.nb_v as usize;
    let n0 = rec.surface0_dims[0] as usize * rec.surface0_dims[1] as usize;
    let n1 = rec.surface1_dims[0] as usize * rec.surface1_dims[1] as usize;
    let pts = rec.points.addr();
    let or_none = |at: u32, n: usize| if at == 0 { 0 } else { n };
    Surface {
        points: (0..or_none(pts, n) as u32).map(|i| [m.f32(pts + 16 * i), m.f32(pts + 16 * i + 4), m.f32(pts + 16 * i + 8), m.f32(pts + 16 * i + 12)]).collect(),
        tags: words(m, rec.tags.addr(), or_none(rec.tags.addr(), n)),
        surface0: vectors(m, rec.surface0.addr(), or_none(rec.surface0.addr(), n0)),
        modif0: words(m, rec.modif0.addr(), or_none(rec.modif0.addr(), n0)),
        surface1: vectors(m, rec.surface1.addr(), or_none(rec.surface1.addr(), n1)),
        modif1: words(m, rec.modif1.addr(), or_none(rec.modif1.addr(), n1)),
        subdiv0: Vec::new(),
        subdiv1: Vec::new(),
        rec,
    }
}

/// Stores what a value form may have changed of a patch loaded by [`load_surface`]: the
/// record (its pointers are the image's own, untouched by the value forms that go
/// through here), the control points and the four display arrays.
pub(crate) fn store_surface(m: &mut Mem, p: Ptr32<Patch>, s: &Surface) {
    let before = m.read(p);
    let mut rec = s.rec;
    (rec.points, rec.tags, rec.surface0, rec.surface1) = (before.points, before.tags, before.surface0, before.surface1);
    (rec.modif0, rec.modif1, rec.subdiv0, rec.subdiv1) = (before.modif0, before.modif1, before.subdiv0, before.subdiv1);
    m.write(p, rec);
    for (i, q) in s.points.iter().enumerate() {
        for k in 0..4 {
            m.set_f32(rec.points.addr() + 16 * i as u32 + 4 * k, q[k as usize]);
        }
    }
    for (at, list) in [(rec.surface0.addr(), &s.surface0), (rec.surface1.addr(), &s.surface1)] {
        for (i, v) in list.iter().enumerate() {
            m.set_f32(at + 12 * i as u32, v.x);
            m.set_f32(at + 12 * i as u32 + 4, v.y);
            m.set_f32(at + 12 * i as u32 + 8, v.z);
        }
    }
    for (at, list) in [(rec.modif0.addr(), &s.modif0), (rec.modif1.addr(), &s.modif1)] {
        for (i, v) in list.iter().enumerate() {
            m.set_i16(at + 2 * i as u32, *v);
        }
    }
}

/// `SI_patchComputeSurface` (0x0847c1d0): the display curves of a patch, through the
/// value form [`crate::patch_grid::compute_surface`] (cited and compared there).
///
/// The routine allocates nothing, so the heap is the original's either way; what the
/// adapter does not reproduce is memory outside the patch's arrays: where the original
/// reads or writes past a block the value form reads zeros and writes nothing.
///
/// Where a capping the routine would run leaves the block of points (a capped border on
/// a patch with too few rows), the adapter cannot follow: it records a fault at
/// [`ADAPTER_LIMIT`] (an address that is no memory) and changes nothing.
// adapter: not in place
pub fn patch_compute_surface(m: &mut Mem, p: Ptr32<Patch>, mode: i16) {
    let mut s = load_surface(m, p);
    let r = &s.rec;
    if r.unk_1e4 & 1 == 0 {
        let caps = [(r.v_bottom, r.v_closed, 0), (r.v_top, r.v_closed, 1), (r.u_bottom, r.u_closed, 2), (r.u_top, r.u_closed, 3)];
        if caps.iter().any(|(flag, closed, which)| *flag != 0 && *closed == 0 && !crate::patch_grid::capping_in_block(&s, *which)) {
            m.record_fault(ADAPTER_LIMIT, 0, Access::Read);
            return;
        }
    }
    crate::patch_grid::compute_surface(&mut s, mode);
    store_surface(m, p, &s);
}

/// Frees and rebuilds everything derived from the control points: `SI_patchUpdateSurface`
/// (0x0847cee4; cited at [`crate::patch_grid::update_surface`]), here with its calls:
/// `free` of the six arrays that are not null, in the order +0x1b0, +0x1d8, +0x1b8,
/// +0x1dc, +0x1c4, +0x1c8, each pointer then cleared; the four dimensions (16 bits); six
/// `SI_calloc` in the order +0x1b0 (12 bytes a point), +0x1d8 (2), +0x1b8, +0x1dc, +0x1c4,
/// +0x1c8; then [`patch_compute_surface`] with mode 0.
pub fn patch_update_surface(m: &mut Mem, p: Ptr32<Patch>) {
    for off in [0x1b0u32, 0x1d8, 0x1b8, 0x1dc, 0x1c4, 0x1c8] {
        let a = m.u32(p.addr() + off);
        if a != 0 {
            m.free(a);
            m.set_u32(p.addr() + off, 0);
        }
    }
    // a type outside 0..=3 leaves the routine's two counts unset in the original; 0 here
    let (nsu, _) = nb_segment_step(m[p].u_type, m[p].nb_u as i32, m[p].u_closed);
    let (nsv, _) = nb_segment_step(m[p].v_type, m[p].nb_v as i32, m[p].v_closed);
    let d0 = (m[p].u_step as i32).wrapping_mul(nsu).wrapping_add(1) as u16;
    m[p].surface0_dims[0] = d0;
    let d1 = (m[p].u_nb_curves as i32).wrapping_mul(nsv).wrapping_add((m[p].v_closed == 0) as i32) as u16;
    m[p].surface0_dims[1] = d1;
    let n0 = d0 as u32 * d1 as u32;
    m[p].surface0 = Ptr32::new(m.si_calloc(n0, 12));
    m[p].modif0 = Ptr32::new(m.si_calloc(n0, 2));
    let d0 = (m[p].v_step as i32).wrapping_mul(nsv).wrapping_add(1) as u16;
    m[p].surface1_dims[0] = d0;
    let d1 = (m[p].v_nb_curves as i32).wrapping_mul(nsu).wrapping_add((m[p].u_closed == 0) as i32) as u16;
    m[p].surface1_dims[1] = d1;
    let n1 = d0 as u32 * d1 as u32;
    m[p].surface1 = Ptr32::new(m.si_calloc(n1, 12));
    m[p].modif1 = Ptr32::new(m.si_calloc(n1, 2));
    let n2 = nsu.wrapping_mul(nsv).wrapping_mul(m[p].u_step as i32 + 1).wrapping_mul(m[p].v_step as i32 + 1) as u32;
    m[p].subdiv0 = Ptr32::new(m.si_calloc(n2, 12));
    m[p].subdiv1 = Ptr32::new(m.si_calloc(n2, 12));
    patch_compute_surface(m, p, 0);
}

fn point_at(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32) << 4)
}

fn tag_at(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32) << 1)
}

fn set_table(m: &mut Mem, table: u32, i: i32, v: i32) {
    m.set_i32(table.wrapping_add((i as u32) << 2), v);
}

/// Adds a row of control points beside a picked display point
/// [soft 0x08482ca8 SI_patchAddRow]. `index` is a point's index in the display curves
/// (the U-running set first, then the V-running set); `along_v` non-zero adds a row in V
/// (every U row gets one more point), zero a row in U. `table`, when not null, records
/// where each old point went.
///
/// **In V.** The place along a V-running curve is `r = (index − points of the U-running
/// set) mod (+0x1bc)`, and from it the row before which to insert: linear `r`; Bézier
/// `3 × ((r − 1) / v_step) + 1`, with **three rows** added (three passes); B-spline and
/// cardinal `(r − 1) / v_step + 2`, or 1 when V is closed and that is beyond the rows.
/// Each pass builds new arrays (`SI_calloc(total, 16)`, `SI_calloc(total, 2)`): the points
/// up to the place are copied, the others move up by one for every row of `nb_v + 1`
/// passed, and each new point is the mean of its two neighbours in its row (the next one
/// wrapping to the row's start when either direction is closed and the new point is the
/// row's last), fourth float 1.0, tag 0. Then `nb_v` goes up, the old arrays are freed
/// and the place moves on by 2. `total` is the final count from the first pass on, so the
/// first two Bézier passes copy points from beyond the old array.
///
/// The mean is `(a + b) × 0.5`: for x the sum stays in the register; for y and z it is
/// rounded to float before the product.
///
/// **In U.** The row is `(index mod (+0x1b4)) × nb_v` (linear), `nb_v × 3 × ((r − 1) /
/// u_step + 1)` (Bézier, three passes), `nb_v × ((r − 1) / u_step + 2)` (the others; back
/// to `nb_v` when U is closed and that is beyond the points). Both arrays are grown in
/// place (`SI_realloc`), the rows from the place on move up by `nb_v`, and the new row is
/// the mean of the row before and the row that was at the place (the first row when
/// either direction is closed and the place is the end): every coordinate
/// `float(0.5 × float(a + b))`. Each pass moves the place back by `nb_v`.
///
/// A type outside 0..=3 leaves the place unset (an uninitialised local in the original;
/// the image's stack fill here). The index table of a Bézier direction is written by
/// separate loops at the end, reproduced as they are. Ends with
/// [`patch_update_surface`].
pub fn patch_add_row(m: &mut Mem, p: Ptr32<Patch>, index: i32, along_v: i16, table: u32) {
    let mut nu = m[p].nb_u as i32;
    let mut nv = m[p].nb_v as i32;
    // the place: a local nobody sets for a type outside 0..=3
    let slot = m.local(4);
    let mut pos = m.i32(slot);
    m.release(4);
    if along_v != 0 {
        let mut total = nu.wrapping_mul(nv).wrapping_add(nu);
        let mut reps = 1;
        let d0 = m[p].surface0_dims;
        let (_, r) = idiv(m, index.wrapping_sub(d0[0] as i32 * d0[1] as i32), m[p].surface1_dims[0] as i32);
        match m[p].v_type {
            0 => pos = r,
            1 => {
                let (q, _) = idiv(m, r - 1, m[p].v_step as i32);
                total = (nv + 3).wrapping_mul(nu);
                pos = q.wrapping_mul(3).wrapping_add(1);
                reps = 3;
            }
            2 | 3 => {
                let (q, _) = idiv(m, r - 1, m[p].v_step as i32);
                pos = q.wrapping_add(2);
                if m[p].v_closed != 0 && pos > nv {
                    pos = 1;
                }
            }
            _ => {}
        }
        let mut k = 0;
        while k < reps {
            let new_points = m.si_calloc(total as u32, POINT);
            let new_tags = m.si_calloc(total as u32, 2);
            let nv1 = nv.wrapping_add(1);
            let mut i = 0;
            while i <= pos && m.fault().is_none() {
                m.copy(point_at(new_points, i), point_at(m[p].points.addr(), i), POINT);
                let t = m.u16(tag_at(m[p].tags.addr(), i));
                m.set_u16(tag_at(new_tags, i), t);
                i += 1;
            }
            let mut shift = 1;
            let mut i = pos.wrapping_add(1);
            while i < total && m.fault().is_none() {
                let (_, rem) = idiv(m, i.wrapping_sub(pos), nv1);
                if rem != 0 {
                    let from = i.wrapping_sub(shift);
                    m.copy(point_at(new_points, i), point_at(m[p].points.addr(), from), POINT);
                    let t = m.u16(tag_at(m[p].tags.addr(), from));
                    m.set_u16(tag_at(new_tags, i), t);
                    if table != 0 && k == 0 && m[p].v_type != 1 {
                        set_table(m, table, from, i);
                    }
                } else {
                    shift += 1;
                }
                i += 1;
            }
            let mut i = pos;
            while i < total && m.fault().is_none() {
                let prev = i.wrapping_sub(1);
                let mut next = i.wrapping_add(1);
                if m.u32(p.addr() + 0x10) != 0 {
                    let t = i.wrapping_sub(nv);
                    let (_, rem) = idiv(m, t, nv1);
                    if rem == 0 {
                        next = t;
                    }
                }
                let (a, b, to) = (point_at(new_points, prev), point_at(new_points, next), point_at(new_points, i));
                let sum_x = x(m.f32(a)) + m.f32(b);
                m.set_f32(to, sum_x.to_f32());
                let y = (x(m.f32(a + 4)) + m.f32(b + 4)).to_f32();
                m.set_f32(to + 4, y);
                let z = (x(m.f32(a + 8)) + m.f32(b + 8)).to_f32();
                m.set_f32(to + 8, z);
                m.set_f32(to, (x(0.5f64) * sum_x).to_f32());
                let y = (x(0.5f64) * m.f32(to + 4)).to_f32();
                m.set_f32(to + 4, y);
                let z = (x(0.5f64) * m.f32(to + 8)).to_f32();
                m.set_f32(to + 12, 1.0);
                m.set_f32(to + 8, z);
                m.set_u16(tag_at(new_tags, i), 0);
                i = i.wrapping_add(1).wrapping_add(nv);
            }
            m[p].nb_v = m[p].nb_v.wrapping_add(1);
            let old = m[p].points.addr();
            if old != 0 {
                m.free(old);
                m[p].points = Ptr32::NULL;
            }
            let old = m[p].tags.addr();
            if old != 0 {
                m.free(old);
                m[p].tags = Ptr32::NULL;
            }
            m[p].points = Ptr32::new(new_points);
            m[p].tags = Ptr32::new(new_tags);
            nv = nv1;
            k += 1;
            pos = pos.wrapping_add(2);
        }
        if table != 0 && m[p].v_type == 1 {
            // where the points of a Bézier V went: three descending sweeps over the rows
            let all = nu.wrapping_mul(nv);
            pos = pos.wrapping_sub(2);
            let mut i = all.wrapping_sub(1);
            let mut b = nu;
            let mut j = 0;
            if j < nu {
                let width = nv.wrapping_sub(pos);
                loop {
                    let mut c = i.wrapping_sub(b.wrapping_mul(3));
                    if width - 1 > 0 {
                        let mut count = width - 1;
                        loop {
                            set_table(m, table, c, i);
                            i = i.wrapping_sub(1);
                            c = c.wrapping_sub(1);
                            count -= 1;
                            if count == 0 || m.fault().is_some() {
                                break;
                            }
                        }
                    }
                    j += 1;
                    b -= 1;
                    i = i.wrapping_sub(1).wrapping_sub(pos);
                    if j >= nu {
                        break;
                    }
                }
            }
            let mut i = pos.wrapping_add(all.wrapping_sub(nv)).wrapping_sub(1);
            let mut b = nu;
            let mut j = 0;
            if j < nu {
                loop {
                    let mut c = i.wrapping_sub(b.wrapping_mul(3)).wrapping_add(1);
                    for _ in 0..2 {
                        set_table(m, table, c, i);
                        i = i.wrapping_sub(2);
                        c = c.wrapping_sub(1);
                    }
                    j += 1;
                    i = i.wrapping_add(4).wrapping_sub(nv);
                    b -= 1;
                    if j >= nu {
                        break;
                    }
                }
            }
            let mut i = pos.wrapping_add(all.wrapping_sub(nv)).wrapping_sub(5);
            let mut b = nu - 1;
            let mut j = 0;
            if j < nu - 1 {
                let run = pos.wrapping_sub(4);
                let width = nv.wrapping_sub(pos);
                loop {
                    let mut c = i.wrapping_sub(b.wrapping_mul(3));
                    if run > 0 {
                        let mut count = run;
                        loop {
                            set_table(m, table, c, i);
                            i = i.wrapping_sub(1);
                            c = c.wrapping_sub(1);
                            count -= 1;
                            if count == 0 || m.fault().is_some() {
                                break;
                            }
                        }
                    }
                    j += 1;
                    i = i.wrapping_sub(4).wrapping_sub(width);
                    b -= 1;
                    if j >= nu - 1 {
                        break;
                    }
                }
            }
        }
    } else {
        let mut count = nu.wrapping_mul(nv);
        let mut total = nv.wrapping_add(count);
        let mut reps = 1;
        let mut first_place = 0;
        let ut = m[p].u_type;
        if (0..=3).contains(&ut) {
            let (_, r) = idiv(m, index, m[p].surface0_dims[0] as i32);
            match ut {
                0 => pos = r.wrapping_mul(nv),
                1 => {
                    let (q, _) = idiv(m, r - 1, m[p].u_step as i32);
                    pos = nv.wrapping_mul(3).wrapping_mul(q.wrapping_add(1));
                    total = (nu + 3).wrapping_mul(nv);
                    reps = 3;
                    first_place = pos;
                }
                _ => {
                    let (q, _) = idiv(m, r - 1, m[p].u_step as i32);
                    pos = nv.wrapping_mul(q.wrapping_add(2));
                    if m[p].u_closed != 0 && pos > count {
                        pos = nv;
                    }
                }
            }
        }
        let grown = m.si_realloc(m[p].points.addr(), (total as u32) << 4);
        m[p].points = Ptr32::new(grown);
        let grown = m.si_realloc(m[p].tags.addr(), (total as u32).wrapping_add(total as u32));
        m[p].tags = Ptr32::new(grown);
        let mut k = 0;
        while k < reps {
            count = nu.wrapping_mul(nv);
            let before = pos.wrapping_sub(nv);
            let mut i = count.wrapping_sub(1);
            while i >= pos && m.fault().is_none() {
                let to = i.wrapping_add(nv);
                let pts = m[p].points.addr();
                m.copy(point_at(pts, to), point_at(pts, i), POINT);
                let tags = m[p].tags.addr();
                let t = m.u16(tag_at(tags, i));
                m.set_u16(tag_at(tags, to), t);
                if table != 0 && m[p].u_type != 1 {
                    set_table(m, table, i, to);
                }
                i = i.wrapping_sub(1);
            }
            let mut next = pos;
            if m.u32(p.addr() + 0x10) != 0 && pos == count {
                next = 0;
            }
            let mut j = 0;
            while j < nv && m.fault().is_none() {
                let pts = m[p].points.addr();
                let (a, b, to) = (point_at(pts, before.wrapping_add(j)), point_at(pts, next.wrapping_add(j)), point_at(pts, pos.wrapping_add(j)));
                for c in [0u32, 4, 8] {
                    let v = (x(m.f32(a + c)) + m.f32(b + c)).to_f32();
                    m.set_f32(to + c, v);
                }
                for c in [0u32, 4, 8] {
                    let v = (x(0.5f64) * m.f32(to + c)).to_f32();
                    m.set_f32(to + c, v);
                }
                m.set_f32(to + 12, 1.0);
                let tags = m[p].tags.addr();
                m.set_u16(tag_at(tags, pos.wrapping_add(j)), 0);
                j += 1;
            }
            m[p].nb_u = m[p].nb_u.wrapping_add(1);
            nu = nu.wrapping_add(1);
            k += 1;
            pos = before;
        }
        if table != 0 && m[p].u_type == 1 {
            // where the rows of a Bézier U went: by 3, 2 and 1 rows
            let mut i = (nu - 3).wrapping_mul(nv).wrapping_sub(1);
            let two = nv.wrapping_add(nv);
            for (limit, by) in [(first_place, nv.wrapping_add(two)), (first_place.wrapping_sub(nv), two), (first_place.wrapping_sub(two), nv)] {
                while i >= limit && m.fault().is_none() {
                    set_table(m, table, i, i.wrapping_add(by));
                    i = i.wrapping_sub(1);
                }
            }
        }
    }
    patch_update_surface(m, p);
}

/// `floor(q / n + 0.5)` as `SI_patchCut` computes it: both integers loaded, the quotient
/// in the register, the double 0.5 added, `frndint` under round-down, stored as a double
/// and converted with truncation.
fn half_up(q: i32, n: i32) -> i32 {
    use si_x87::{Round, F80};
    let v = ((F80::from_i32(q) / F80::from_i32(n)) + 0.5f64).round_int(Round::Down).to_f64();
    x(v).to_i32(Round::Zero)
}

fn replace_arrays(m: &mut Mem, p: Ptr32<Patch>, points: u32, tags: u32) {
    let old = m[p].points.addr();
    if old != 0 {
        m.free(old);
        m[p].points = Ptr32::NULL;
    }
    let old = m[p].tags.addr();
    if old != 0 {
        m.free(old);
        m[p].tags = Ptr32::NULL;
    }
    m[p].points = Ptr32::new(points);
    m[p].tags = Ptr32::new(tags);
}

/// Cuts a patch in two at a picked display curve [soft 0x08483544 SI_patchCut]: `a` gets
/// the part before the cut, `b` the part from it on; both are patches the caller made
/// (copies of `src`: the routine sets only their point count in the cut direction and
/// their arrays). `*want_a` and `*want_b` say which parts to build; `table_a` and
/// `table_b`, when not null, get each old point's index in its part.
///
/// - `index` up to the number of points of the U-running display set picks one of those
///   curves: a cut **in V**. With `q = index / (+0x1b4)` the curve's number, the cut row
///   is `c = floor(q / u_nb_curves + 0.5)`, times 3 for a Bézier V; the result is 1 when
///   `q` is not a multiple of `u_nb_curves` (the cut is not on a control row), else 0.
///   `a.nb_v = c + margin`, `b.nb_v = nb_v − c`, with `margin` 1 (linear, Bézier) or 3
///   (B-spline, cardinal: the rows the curve needs beyond the cut). Each wanted part gets
///   new arrays (`SI_calloc(n, 16)`, `SI_calloc(n, 2)`) filled from `src` by walking all
///   its points: `a` takes those whose column is below `a.nb_v`, `b` those from column
///   `c` on; the old arrays are freed and the display rebuilt.
/// - a larger `index` picks a V-running curve: a cut **in U**, the same with
///   `(index − that number) / (+0x1bc)` and `v_nb_curves`. Here `a` keeps its own arrays,
///   shortened in place (`SI_realloc`; the table gets the identity), and `b` is filled
///   with the last `b.nb_u` rows of `src`.
/// - A cut at row 0, or at `count − margin`, is refused: both flags are cleared and the
///   result is 0.
///
/// For a type outside 0..=3 `margin` is a register the routine never sets.
#[allow(clippy::too_many_arguments)]
pub fn patch_cut(m: &mut Mem, src: Ptr32<Patch>, a: Ptr32<Patch>, b: Ptr32<Patch>, want_a: u32, want_b: u32, index: i32, table_a: u32, table_b: u32) -> i32 {
    let nu = m[src].nb_u as i32;
    let nv = m[src].nb_v as i32;
    let d0 = m[src].surface0_dims[0] as i32;
    let n0 = d0.wrapping_mul(m[src].surface0_dims[1] as i32);
    let slot = m.local(4);
    let mut margin = m.i32(slot);
    m.release(4);
    let in_v = index <= n0;
    let (q, curves) = if in_v {
        (idiv(m, index, d0).0, m[src].u_nb_curves as i32)
    } else {
        (idiv(m, index.wrapping_sub(n0), m[src].surface1_dims[0] as i32).0, m[src].v_nb_curves as i32)
    };
    let off_row = (idiv(m, q, curves).1 != 0) as i32;
    let mut c = half_up(q, curves);
    match if in_v { m[src].v_type } else { m[src].u_type } {
        0 => margin = 1,
        1 => {
            c = c.wrapping_mul(3);
            margin = 1;
        }
        2 | 3 => margin = 3,
        _ => {}
    }
    if c == 0 || c == (if in_v { nv } else { nu }).wrapping_sub(margin) {
        m.set_i16(want_b, 0);
        m.set_i16(want_a, 0);
        return 0;
    }
    if in_v {
        m[a].nb_v = c.wrapping_add(margin) as u16;
        m[b].nb_v = nv.wrapping_sub(c) as u16;
        if m.i16(want_a) != 0 {
            let n = m[a].nb_u as u32 * m[a].nb_v as u32;
            let points = m.si_calloc(n, POINT);
            let tags = m.si_calloc(n, 2);
            let total = nu.wrapping_mul(nv);
            let mut out = 0;
            let mut i = 0;
            while i < total && m.fault().is_none() {
                let (_, col) = idiv(m, i, nv);
                if col <= m[a].nb_v as i32 - 1 {
                    m.copy(point_at(points, out), point_at(m[src].points.addr(), i), POINT);
                    let t = m.u16(tag_at(m[src].tags.addr(), i));
                    m.set_u16(tag_at(tags, out), t);
                    if table_a != 0 {
                        set_table(m, table_a, i, out);
                    }
                    out += 1;
                }
                i += 1;
            }
            replace_arrays(m, a, points, tags);
            patch_update_surface(m, a);
        }
        if m.i16(want_b) != 0 {
            let n = m[b].nb_u as u32 * m[b].nb_v as u32;
            let points = m.si_calloc(n, POINT);
            let tags = m.si_calloc(n, 2);
            let total = nv.wrapping_mul(nu);
            let mut out = 0;
            let mut i = 0;
            while i < total && m.fault().is_none() {
                let (_, col) = idiv(m, i, nv);
                if col >= c {
                    m.copy(point_at(points, out), point_at(m[src].points.addr(), i), POINT);
                    let t = m.u16(tag_at(m[src].tags.addr(), i));
                    m.set_u16(tag_at(tags, out), t);
                    if table_b != 0 {
                        set_table(m, table_b, i, out);
                    }
                    out += 1;
                }
                i += 1;
            }
            replace_arrays(m, b, points, tags);
            patch_update_surface(m, b);
        }
    } else {
        m[a].nb_u = c.wrapping_add(margin) as u16;
        m[b].nb_u = nu.wrapping_sub(c) as u16;
        if m.i16(want_a) != 0 {
            let n = (m[a].nb_u as u32).wrapping_mul(m[a].nb_v as u32);
            let grown = m.si_realloc(m[a].points.addr(), n << 4);
            m[a].points = Ptr32::new(grown);
            let grown = m.si_realloc(m[a].tags.addr(), n.wrapping_add(n));
            m[a].tags = Ptr32::new(grown);
            if table_a != 0 {
                let mut i = 0;
                while i < n as i32 && m.fault().is_none() {
                    set_table(m, table_a, i, i);
                    i += 1;
                }
            }
            patch_update_surface(m, a);
        }
        if m.i16(want_b) != 0 {
            let n = (m[b].nb_u as u32 * m[b].nb_v as u32) as i32;
            let points = m.si_calloc(n as u32, POINT);
            let tags = m.si_calloc(n as u32, 2);
            let mut from = (m[b].nb_v as i32).wrapping_mul(nu.wrapping_sub(m[b].nb_u as i32));
            let mut out = 0;
            while out < n && m.fault().is_none() {
                m.copy(point_at(points, out), point_at(m[src].points.addr(), from), POINT);
                let t = m.u16(tag_at(m[src].tags.addr(), from));
                m.set_u16(tag_at(tags, out), t);
                if table_b != 0 {
                    set_table(m, table_b, from, out);
                }
                out += 1;
                from = from.wrapping_add(1);
            }
            replace_arrays(m, b, points, tags);
            patch_update_surface(m, b);
        }
    }
    off_row
}

/// A new patch [soft 0x0847a0d0 SI_patchAllocate]: `SI_malloc(0x204)` and the fields the
/// routine sets, in its order. **Not set** (they hold what the allocator gave): the two
/// basis matrices (+0x24, +0x64), the word at +0x1e0 and the padding. Null when the
/// allocation fails.
pub fn patch_allocate(m: &mut Mem) -> Ptr32<Patch> {
    let p: Ptr32<Patch> = Ptr32::new(m.si_malloc(Patch::SIZE as u32));
    if p.is_null() {
        return p;
    }
    m[p].u_tension = 0.0;
    m[p].v_tension = 0.0;
    m[p].unk_000 = 0;
    m[p].u_type = 0;
    m[p].v_type = 0;
    m[p].nb_u = 0;
    m[p].nb_v = 0;
    m[p].u_closed = 0;
    m[p].v_closed = 0;
    m[p].u_step = 3;
    m[p].v_step = 3;
    m[p].u_nb_curves = 1;
    m[p].v_nb_curves = 1;
    for k in 0..4 {
        // SI_matrixCopy from the global
        let identity = si_math::algebra::matrix_copy(&m[crate::spline_image::SI_IDENTITE]);
        m[p].matrices[k] = identity;
    }
    m[p].mr_edge = 5.0;
    m[p].mr_ucurv = 0.1;
    m[p].mr_vcurv = 45.0;
    m[p].points = Ptr32::NULL;
    m[p].tags = Ptr32::NULL;
    m[p].unk_1ac = 0;
    m[p].surface0 = Ptr32::NULL;
    m[p].modif0 = Ptr32::NULL;
    m[p].surface0_dims = [0, 0];
    m[p].surface1 = Ptr32::NULL;
    m[p].modif1 = Ptr32::NULL;
    m[p].surface1_dims = [0, 0];
    m[p].unk_1c0 = 0;
    m[p].subdiv0 = Ptr32::NULL;
    m[p].subdiv1 = Ptr32::NULL;
    m[p].u_bottom = 0;
    m[p].u_top = 0;
    m[p].v_bottom = 0;
    m[p].v_top = 0;
    m[p].unk_1d4 = 0;
    m[p].unk_1d6 = 0;
    m[p].mr_approx = 1;
    m[p].mr_view = 0;
    m[p].mr_min = 1;
    m[p].mr_max = 10;
    m[p].mr_rec = 0;
    m[p].unk_1e4 = 0;
    p
}

/// A copy of a patch [soft 0x0847a2b8 SI_patchCopy], with its calls: [`patch_allocate`]
/// (null ends the routine), the fields copied one by one (the floats through the unit, so
/// a signalling NaN arrives quiet), the six matrices (`SI_matrixCopy`),
/// `SI_calloc(nb_u × nb_v, 16)` and `SI_calloc(nb_u × nb_v, 2)` filled from the source's
/// points and tags (no test of either pointer), and [`patch_update_surface`] on the copy.
/// Not copied: the words at +0x000 and +0x1e0, the display arrays and the shading grid.
pub fn patch_copy(m: &mut Mem, src: Ptr32<Patch>) -> Ptr32<Patch> {
    use si_math::algebra::matrix_copy;
    use si_x87::fmov;
    let n = patch_allocate(m);
    if n.is_null() {
        return n;
    }
    // the source is not written while the fields go across: one read of it
    let a = m.read(src);
    m[n].unk_1e4 = a.unk_1e4;
    m[n].u_type = a.u_type;
    m[n].v_type = a.v_type;
    m[n].nb_u = a.nb_u;
    m[n].nb_v = a.nb_v;
    m[n].u_closed = a.u_closed;
    m[n].v_closed = a.v_closed;
    m[n].u_tension = fmov(a.u_tension);
    m[n].v_tension = fmov(a.v_tension);
    m[n].u_step = a.u_step;
    m[n].v_step = a.v_step;
    m[n].u_nb_curves = a.u_nb_curves;
    m[n].v_nb_curves = a.v_nb_curves;
    m[n].u_top = a.u_top;
    m[n].u_bottom = a.u_bottom;
    m[n].v_top = a.v_top;
    m[n].v_bottom = a.v_bottom;
    m[n].unk_1d4 = a.unk_1d4;
    m[n].unk_1d6 = a.unk_1d6;
    m[n].unk_1ac = a.unk_1ac;
    m[n].mr_approx = a.mr_approx;
    m[n].mr_view = a.mr_view;
    m[n].mr_edge = fmov(a.mr_edge);
    m[n].mr_ucurv = fmov(a.mr_ucurv);
    m[n].mr_vcurv = fmov(a.mr_vcurv);
    m[n].mr_min = a.mr_min;
    m[n].mr_max = a.mr_max;
    m[n].mr_rec = a.mr_rec;
    m[n].basis = [matrix_copy(&a.basis[0]), matrix_copy(&a.basis[1])];
    m[n].matrices = core::array::from_fn(|i| matrix_copy(&a.matrices[i]));
    let count = a.nb_u as u32 * a.nb_v as u32;
    let points = m.si_calloc(count, POINT);
    m[n].points = Ptr32::new(points);
    let tags = m.si_calloc(count, 2);
    m[n].tags = Ptr32::new(tags);
    let (from_points, from_tags) = (a.points.addr(), a.tags.addr());
    let mut i = 0;
    while i < count && m.fault().is_none() {
        m.copy(points.wrapping_add(i << 4), from_points.wrapping_add(i << 4), POINT);
        let t = m.u16(from_tags.wrapping_add(i << 1));
        m.set_u16(tags.wrapping_add(i << 1), t);
        i += 1;
    }
    patch_update_surface(m, n);
    n
}

/// Frees a patch [soft 0x0847a530 SI_patchDispose]: `*pp` not null: `free` of each array
/// that is set, its pointer then cleared, in the order points, tags, +0x1b0, +0x1d8,
/// +0x1b8, +0x1dc, +0x1c4, +0x1c8; then the record, and `*pp` cleared.
pub fn patch_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    for off in [0x1a4u32, 0x1a8, 0x1b0, 0x1d8, 0x1b8, 0x1dc, 0x1c4, 0x1c8] {
        let a = m.u32(m.u32(pp).wrapping_add(off));
        if a != 0 {
            m.free(a);
            let p = m.u32(pp);
            m.set_u32(p.wrapping_add(off), 0);
        }
    }
    let p = m.u32(pp);
    if p != 0 {
        m.free(p);
        m.set_u32(pp, 0);
    }
}

/// `SI_patchTransferPoints` (0x084810cc): the control points and both sets of display
/// curves of `from` copied into `to`, through the value form
/// [`crate::patch::transfer_points`] (cited and compared there) on copies of the two
/// patches. Returns whether all of that was copied.
///
/// The routine allocates nothing and writes only into `to`'s three arrays. The adapter
/// loads each array as long as its record says (points: `nb_u × nb_v`; display curves:
/// the stored dimensions), while the original copies as many as `from`'s types and steps
/// give:
/// - more points than either patch holds: the adapter cannot follow; it records a fault
///   at [`ADAPTER_LIMIT`] and changes nothing;
/// - more display curve points than one of the four display arrays holds (on the render's
///   image a loaded patch has none: its loader does not run `SI_patchUpdateSurface`): the
///   points are copied and the display curves left as they are; the answer is `false`.
// adapter: not in place
pub fn patch_transfer_points(m: &mut Mem, from: Ptr32<Patch>, to: Ptr32<Patch>) -> bool {
    let src = load_surface(m, from);
    let mut dst = load_surface(m, to);
    let r = &src.rec;
    let ((nsu, _), (nsv, _)) = src.cells();
    let n = r.nb_u as i64 * r.nb_v as i64;
    let n0 = (r.u_step as i64 * nsu as i64 + 1) * (r.u_nb_curves as i64 * nsv as i64 + (r.v_closed == 0) as i64);
    let n1 = (r.v_step as i64 * nsv as i64 + 1) * (r.v_nb_curves as i64 * nsu as i64 + (r.u_closed == 0) as i64);
    let fits = |k: i64, a: usize, b: usize| (0..=a.min(b) as i64).contains(&k);
    if !fits(n, src.points.len(), dst.points.len()) {
        m.record_fault(ADAPTER_LIMIT, 0, Access::Write);
        return false;
    }
    let curves = fits(n0, src.surface0.len(), dst.surface0.len()) && fits(n1, src.surface1.len(), dst.surface1.len());
    if !curves {
        // the value form stops at the end of `to`'s arrays: empty ones, the points only
        (dst.surface0, dst.surface1) = (Vec::new(), Vec::new());
    }
    crate::patch::transfer_points(&src, &mut dst);
    store_surface(m, to, &dst);
    curves
}

/// Removes one row of control points [soft 0x08482958 SI_patchDeleteRow]. `index` is a
/// control point's index; `along` zero removes its column (`index mod nb_v`: every U row
/// loses one point), non-zero its row (`index / nb_v`). A direction with one row or none
/// is refused (0). New arrays (`SI_calloc(n, 16)`, `SI_calloc(n, 2)`, `n` the count after
/// the removal) take every point that is not in the row, in order; `table`, when not
/// null, gets each old point's new index, or −1. The count goes down, the old arrays are
/// freed, and the display is rebuilt ([`patch_update_surface`]). Returns 1.
///
/// The counters are 16-bit; the row is compared as a 16-bit number, so a negative
/// remainder (a negative index) matches no column: every point is then copied into
/// arrays one row too small, as in the original.
pub fn patch_delete_row(m: &mut Mem, p: Ptr32<Patch>, index: i16, along: i16, table: u32) -> i32 {
    let by_row = along != 0;
    if (if by_row { m[p].nb_u } else { m[p].nb_v }) <= 1 {
        return 0;
    }
    let (q, r) = idiv(m, index as i32, m[p].nb_v as i32);
    let row = if by_row { q as u16 } else { r as u16 };
    let n = if by_row { (m[p].nb_u as u32).wrapping_sub(1).wrapping_mul(m[p].nb_v as u32) } else { (m[p].nb_u as u32).wrapping_mul((m[p].nb_v as u32).wrapping_sub(1)) };
    let new_points = m.si_calloc(n, POINT);
    let new_tags = m.si_calloc(n, 2);
    let (mut from, mut from_tag) = (m[p].points.addr(), m[p].tags.addr());
    let (mut to, mut to_tag) = (new_points, new_tags);
    let (mut new_i, mut old_i, mut i) = (0i16, 0i16, 0u16);
    if m[p].nb_u != 0 {
        loop {
            let mut j = 0u16;
            if 0 < m[p].nb_v as i32 {
                loop {
                    if (if by_row { i } else { j }) != row {
                        m.copy(to, from, POINT);
                        let t = m.u16(from_tag);
                        m.set_u16(to_tag, t);
                        to = to.wrapping_add(POINT);
                        to_tag = to_tag.wrapping_add(2);
                        if table != 0 {
                            set_table(m, table, old_i as i32, new_i as i32);
                        }
                        new_i = new_i.wrapping_add(1);
                    } else if table != 0 {
                        set_table(m, table, old_i as i32, -1);
                    }
                    j = j.wrapping_add(1);
                    from = from.wrapping_add(POINT);
                    from_tag = from_tag.wrapping_add(2);
                    old_i = old_i.wrapping_add(1);
                    if j as i16 as i32 >= m[p].nb_v as i32 || m.fault().is_some() {
                        break;
                    }
                }
            }
            i = i.wrapping_add(1);
            if i as i16 as i32 >= m[p].nb_u as i32 || m.fault().is_some() {
                break;
            }
        }
    }
    if by_row {
        m[p].nb_u = m[p].nb_u.wrapping_sub(1);
    } else {
        m[p].nb_v = m[p].nb_v.wrapping_sub(1);
    }
    replace_arrays(m, p, new_points, new_tags);
    patch_update_surface(m, p);
    1
}

/// What `SI_bezierShrink` (the hrc package's image form) calls outside its source file:
/// the spline matrix update, which lives in [`crate::spline_image`]. No stream is used.
struct ShrinkOutside;

impl si_hrc::image::Outside for ShrinkOutside {
    fn fopen(&mut self, _m: &mut Mem, _path: u32, _mode: u32) -> u32 {
        0
    }
    fn fclose(&mut self, _m: &mut Mem, _file: u32) -> i32 {
        0
    }
    fn fread(&mut self, _m: &mut Mem, _ptr: u32, _size: u32, _n: u32, _file: u32) -> u32 {
        0
    }
    fn fgetc(&mut self, _m: &mut Mem, _file: u32) -> i32 {
        -1
    }
    fn rewind(&mut self, _m: &mut Mem, _file: u32) {}
    fn bytes_left(&mut self, _m: &mut Mem, _file: u32) -> u64 {
        0
    }
    fn call(&mut self, m: &mut Mem, routine: &'static si_hrc::image::Routine, args: &[u32]) -> u32 {
        if routine.addr == 0x0848_abd0 {
            crate::spline_image::spline_update_matrix(m, Ptr32::new(args[0]));
        }
        0
    }
}

/// The spline along one direction of a patch at a parameter of the other
/// [soft 0x0847d764 SI_patchExtractSpline]: `along_v` 0 runs the spline in U (the patch's
/// U type, count, closed flag, tension and step; the weights are V's basis at `t`), any
/// other value in V.
///
/// Read from the disassembly: the whole part of `t` is peeled off in single precision
/// (`t = float(t - 1)` while `1.0 < t`, a NaN ends the loop at once), counting the turns
/// and adding the cross direction's step to the first control point each turn; the four
/// weights are formed in the register from the transposed cross basis and `t`, `t²`, `t³`
/// (kept unrounded), each stored as a `float`; each key's position is
/// `float(((p₂w₂ + (p₁w₁ + p₀w₀)) + p₃w₃))` per coordinate. Rows two to four wrap by the
/// count of **turns** against the row count less 2, 3 and 4. A Bézier result is replaced
/// by `SI_bezierShrink` of it and the first spline is disposed. A failed
/// `SI_splineAllocate` answers null.
///
/// Rows are read where the arithmetic puts them, whatever the block holds. Where the
/// original's peeling never ends (`t` above 2²⁴, or infinite) a fault is recorded at
/// [`ADAPTER_LIMIT`] and the answer is null; a Bézier or other result is not otherwise
/// touched. A cross type outside 0..=3 leaves the basis unset (zeros here).
pub fn patch_extract_spline(m: &mut Mem, p: Ptr32<Patch>, along_v: i16, t: f32) -> Ptr32<Spline> {
    let spl = spline_allocate(m);
    if spl.is_null() {
        return spl;
    }
    let pa = p.addr();
    let (cross_type, cross_tension, cross_nb, cross_closed);
    if along_v == 0 {
        // the spline runs in U; the cross direction is V
        m[spl].kind = m.i32(pa + 4);
        m[spl].nb_keys = m.u16(pa + 0xc) as i16;
        m[spl].closed = m.i16(pa + 0x10);
        m[spl].tension = fmov(m.f32(pa + 0x14));
        m[spl].step = m.u16(pa + 0x1c) as i16;
        cross_type = m.i32(pa + 8);
        cross_tension = m.f32(pa + 0x18);
        cross_nb = m.u16(pa + 0xe);
        cross_closed = m.i16(pa + 0x12);
    } else {
        m[spl].kind = m.i32(pa + 8);
        m[spl].nb_keys = m.u16(pa + 0xe) as i16;
        m[spl].closed = m.i16(pa + 0x12);
        m[spl].tension = fmov(m.f32(pa + 0x18));
        m[spl].step = m.u16(pa + 0x1e) as i16;
        cross_type = m.i32(pa + 4);
        cross_tension = m.f32(pa + 0x14);
        cross_nb = m.u16(pa + 0xc);
        cross_closed = m.i16(pa + 0x10);
    }
    let mut b: si_math::types::Matrix = [[0.0; 4]; 4];
    crate::basis::spline_basis_matrix(cross_type, cross_tension, &mut b);
    let (_, step) = nb_segment_step(cross_type, cross_nb as i32, cross_closed);
    // the peeling, in single precision
    let mut t = t;
    let mut sum = 0i32;
    let mut turns = 0i32;
    let mut guard = 0u32;
    while 1.0f32 < t {
        let n = (x(t) - x(1.0f32)).to_f32();
        guard += 1;
        if guard > (1 << 26) || n.to_bits() == t.to_bits() {
            // the original does not return
            m.record_fault(ADAPTER_LIMIT, 0, Access::Read);
            return Ptr32::NULL;
        }
        t = n;
        sum = sum.wrapping_add(step);
        turns += 1;
    }
    let nv = m.u16(pa + 0xe) as i32;
    let (base0, across, next, rows) = if along_v == 0 {
        (sum, 1, nv, nv)
    } else {
        (nv.wrapping_mul(sum), nv, 1, m.u16(pa + 0xc) as i32)
    };
    spline_update_matrix(m, spl);
    let keys = spline_key_allocate(m, m[spl].nb_keys as u16 as i32);
    m[spl].keys = keys;
    let tr = matrix_transpose(&b);
    let t2 = x(t) * t;
    let t3 = t2 * t;
    let w0 = ((((x(tr[0][1]) * t2) + (x(tr[0][0]) * t3)) + (x(t) * tr[0][2])) + tr[0][3]).to_f32();
    let w1 = ((((x(tr[1][1]) * t2) + (x(tr[1][0]) * t3)) + (x(t) * tr[1][2])) + tr[1][3]).to_f32();
    let w2 = ((((x(tr[2][1]) * t2) + (x(tr[2][0]) * t3)) + (x(t) * tr[2][2])) + tr[2][3]).to_f32();
    let w3 = (((x(t) * tr[3][2]) + ((t2 * tr[3][1]) + (t3 * tr[3][0]))) + tr[3][3]).to_f32();
    let (mut o1, mut o2, mut o3) = (across, across.wrapping_mul(2), across.wrapping_mul(3));
    if turns > rows.wrapping_sub(2) {
        o1 = o1.wrapping_sub(across.wrapping_mul(rows));
    }
    if turns > rows.wrapping_sub(3) {
        o2 = o2.wrapping_sub(across.wrapping_mul(rows));
    }
    if turns > rows.wrapping_sub(4) {
        o3 = o3.wrapping_sub(rows.wrapping_mul(across));
    }
    let mut base = base0;
    if m[spl].nb_keys != 0 {
        let mut k = 0u32;
        loop {
            let pts = m.u32(pa + 0x1a4);
            let at = |i: i32| pts.wrapping_add((i as u32).wrapping_shl(4));
            let rows4 = [at(base), at(base.wrapping_add(o1)), at(base.wrapping_add(o2)), at(base.wrapping_add(o3))];
            for c in 0..3u32 {
                let v = |r: usize| x(m.f32(rows4[r].wrapping_add(4 * c)));
                let (p0, p1, p2, p3) = (v(0), v(1), v(2), v(3));
                let out = (((p2 * w2) + ((p1 * w1) + (p0 * w0))) + (p3 * w3)).to_f32();
                m.set_f32(keys.addr().wrapping_add(k * 0x30 + 0xc + 4 * c), out);
            }
            k += 1;
            base = base.wrapping_add(next);
            if (k as i32) >= m[spl].nb_keys as u16 as i32 {
                break;
            }
        }
    }
    if m[spl].kind == 1 {
        let shrunk = si_hrc::image::spline::bezier_shrink(m, &mut ShrinkOutside, spl.addr());
        let sp = m.sp();
        let slot = m.local(4);
        m.set_u32(slot, spl.addr());
        crate::intersect_image::spline_dispose(m, slot);
        m.set_sp(sp);
        return shrunk;
    }
    spl
}
