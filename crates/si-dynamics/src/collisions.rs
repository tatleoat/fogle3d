// SPDX-License-Identifier: MIT
//! `collisions.c` of Softimage|3D 4.0 (0x084a1990 … 0x084a8e03): contact detection between
//! the bodies of the dynamics simulation and the resolution of the contacts.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. The floating-point code was compiled for the x87 unit: the
//! intermediate values of an expression stay in 80-bit registers and are rounded where the
//! original stores them, so the arithmetic goes through [`F80`].

use crate::outside::DynaCalls;
use si_mem::{Mem, Outside};
use si_x87::trig::fpatan;
use si_x87::{x, F80};

/// `fld` of a float in the image.
fn lf(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// The address of float `i` (a signed index) of an array at `base`.
fn at(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32).wrapping_mul(4))
}

/// Gauss-Jordan elimination on the matrix of `rows` rows with `cols` columns (floats at
/// `a`), over the first `n` columns [soft 0x084a1990 resolveLinearSystemRG]: each pivot is
/// swapped with a lower row when its magnitude is below 1e-12 (0 is returned when no row
/// has one), the pivot row is scaled to a unit pivot unless the pivot is within 1e-12 of 1
/// (by the reciprocal rounded to a float), and the other rows lose their multiple of it
/// when their entry in the pivot column exceeds 1e-12. Row and column counters are 16-bit
/// and wrap as the original's do; the row products `i * cols` are done in 16 bits for the
/// swap and in 32 bits elsewhere. Returns 1 when it completes.
pub fn resolve_linear_system_rg(m: &mut Mem, a: u32, rows: i16, cols: i16, n: i16) -> u32 {
    let eps = x(1e-12f64);
    let mut i: i16 = 0;
    while i < n {
        let ii = i as i32;
        let cols32 = cols as i32;
        let pivot_at = at(a, ii * cols32 + ii);
        let next = i.wrapping_add(1);
        if lf(m, pivot_at).abs().lt(eps) {
            let mut j = next;
            while j < rows {
                if !lf(m, at(a, (j as i32) * cols32 + ii)).abs().lt(eps) {
                    break;
                }
                j = j.wrapping_add(1);
            }
            if j == rows {
                return 0;
            }
            let mut dst = i.wrapping_mul(cols);
            let mut src = cols.wrapping_mul(j);
            let mut k = cols;
            if cols > 0 {
                loop {
                    let p = at(a, src as i32);
                    let q = at(a, dst as i32);
                    let (vp, vq) = (lf(m, p).to_f32(), lf(m, q).to_f32());
                    m.set_f32(p, vq);
                    m.set_f32(q, vp);
                    k = k.wrapping_sub(1);
                    dst = dst.wrapping_add(1);
                    src = src.wrapping_add(1);
                    if k == 0 {
                        break;
                    }
                }
            }
        }
        // scale the pivot row to a unit pivot
        let base = ii * cols32;
        let p = lf(m, at(a, ii + base));
        if (p - 1.0f64).abs().gt(eps) {
            let inv = (F80::ONE / p).to_f32();
            let mut c: i16 = 0;
            while c < cols {
                let t = at(a, c as i32 + base);
                m.set_f32(t, (lf(m, t) * inv).to_f32());
                c = c.wrapping_add(1);
            }
        }
        // clear the pivot column of the other rows
        let mut j: i16 = 0;
        while j < rows {
            if j != i {
                let f = lf(m, at(a, (j as i32) * cols32 + ii));
                if f.abs().gt(eps) {
                    let neg = -f;
                    let mut c: i16 = 0;
                    while c < cols {
                        let d = at(a, c as i32 + (j as i32) * cols32);
                        let s = lf(m, at(a, c as i32 + base));
                        m.set_f32(d, (s * neg + lf(m, d)).to_f32());
                        c = c.wrapping_add(1);
                    }
                }
            }
            j = j.wrapping_add(1);
        }
        i = next;
    }
    1
}

/// Offsets of the contact surface record (`SI_SurfOfContact`, at least 0x88 bytes): the
/// points of the polygon where two bodies touch. Read from the code of
/// `surfaceOfContactAddVertex` and `surfaceOfContactComputeArea`; its other fields are
/// named where a function of this file uses them.
pub mod soc {
    /// Number of points.
    pub const COUNT: u32 = 0x00;
    /// Capacity of the point arrays (a multiple of 8).
    pub const CAP: u32 = 0x04;
    /// The points: `COUNT` records of three floats.
    pub const POINTS: u32 = 0x08;
    /// A second array of 12-byte records, kept the size of the first (not touched here).
    pub const SECOND: u32 = 0x0c;
    /// How many times the model a point belongs to changes along the list.
    pub const RUNS: u32 = 0x7c;
    /// The model each point belongs to (`__SI_Model *`, one word a point).
    pub const MODELS: u32 = 0x80;
    /// Set to 0 when the arrays are first made.
    pub const FIELD_84: u32 = 0x84;
}

/// The test of one face of the Liang-Barsky clipping (`clipt`, 0x084a1ba4) on values in
/// registers: returns the answer and which of the interval's ends it stored.
#[allow(clippy::too_many_arguments)]
fn clipt_core(p: f32, q: f32, t0: &mut f32, t1: &mut f32, edge: i32, e0: &mut i32, e1: &mut i32) -> (bool, bool, bool) {
    let zero = F80::ZERO;
    if zero.lt(x(p)) {
        let r = x(q) / x(p);
        if r.gt(x(*t1)) {
            return (false, false, false);
        }
        if r.gt(x(*t0)) {
            *t0 = r.to_f32();
            *e0 = edge;
            return (true, true, false);
        }
        (true, false, false)
    } else if zero.gt(x(p)) {
        let r = x(q) / x(p);
        if r.lt(x(*t0)) {
            return (false, false, false);
        }
        if r.lt(x(*t1)) {
            *t1 = r.to_f32();
            *e1 = edge;
            return (true, false, true);
        }
        (true, false, false)
    } else {
        // p is zero (or not a number): the line is parallel to the face; it is outside
        // when q is positive
        (!x(q).gt(zero), false, false)
    }
}

/// One clipping step of the parametric line against a face [soft 0x084a1ba4 clipt]: `p`
/// is the direction's component (towards the face when negative), `q` the distance to it.
/// The ratio q / p is formed in the register, compared with the interval `[*t0, *t1]` at
/// `t0`, `t1` (floats), and stored as a float when it narrows it, together with `edge`
/// (the face: -1, 1, -2, 2, -3, 3) at `e0` or `e1`. Returns 0 when the line misses the box.
#[allow(clippy::too_many_arguments)]
pub fn clipt(m: &mut Mem, p: f32, q: f32, t0: u32, t1: u32, edge: i32, e0: u32, e1: u32) -> i16 {
    let (mut a, mut b) = (m.f32(t0), m.f32(t1));
    let (mut c, mut d) = (0, 0);
    let (ok, w0, w1) = clipt_core(p, q, &mut a, &mut b, edge, &mut c, &mut d);
    if w0 {
        m.set_f32(t0, a);
        m.set_i32(e0, c);
    }
    if w1 {
        m.set_f32(t1, b);
        m.set_i32(e1, d);
    }
    ok as i16
}

/// Clips the segment from A to B against an axis-aligned box
/// [soft 0x084a1c40 lineBoxLiangBarskyClipping]. `v` is the eighteen floats of the six
/// vectors the original takes by value, in order: A, B, the segment's start S and end E
/// (copied to `p_out` and `q_out` first and shortened to the part inside the box), the
/// box's maximum corner and its minimum corner. `z` receives three zero words (the
/// original zeroes them and does nothing else with them). Returns 2 when a part of
/// at least 0.08 of the segment is inside, 1 when less, 0 when none; when it is 1 and the
/// part starts beyond half of the segment, the start gets the end's coordinates.
///
/// The six differences handed to the clipping steps are rounded to floats, as the original
/// stores them as arguments; the ends of the inside part are computed with the unrounded
/// x difference and the floats of the y and z differences.
pub fn line_box_liang_barsky_clipping(m: &mut Mem, v: [f32; 18], p_out: u32, q_out: u32, z: u32) -> i32 {
    let sub = |a: f32, b: f32| (x(a) - x(b)).to_f32();
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let (mut e0, mut e1) = (0i32, 0i32);
    let mut res = 0;
    for k in 0..3 {
        m.set_u32(p_out + 4 * k, v[6 + k as usize].to_bits());
        m.set_u32(q_out + 4 * k, v[9 + k as usize].to_bits());
    }
    for k in 0..3 {
        m.set_u32(z + 4 * k, 0);
    }
    let (dx, dy, dz) = (sub(v[3], v[0]), sub(v[4], v[1]), sub(v[5], v[2]));
    let ok = clipt_core(dx, sub(v[12], v[0]), &mut t0, &mut t1, -1, &mut e0, &mut e1).0
        && clipt_core(-dx, sub(v[0], v[15]), &mut t0, &mut t1, 1, &mut e0, &mut e1).0
        && clipt_core(dy, sub(v[13], v[1]), &mut t0, &mut t1, -2, &mut e0, &mut e1).0
        && clipt_core(-dy, sub(v[1], v[16]), &mut t0, &mut t1, 2, &mut e0, &mut e1).0
        && clipt_core(dz, sub(v[14], v[2]), &mut t0, &mut t1, -3, &mut e0, &mut e1).0
        && clipt_core(-dz, sub(v[2], v[17]), &mut t0, &mut t1, 3, &mut e0, &mut e1).0;
    if ok {
        res = 2;
        if (x(t1) - x(t0)).lt(x(0.08f64)) {
            res = 1;
        }
        if F80::ONE.gt(x(t1)) || F80::ZERO.lt(x(t0)) {
            let dx2 = x(v[9]) - x(v[6]);
            let dy2 = (x(v[10]) - x(v[7])).to_f32();
            let dz2 = (x(v[11]) - x(v[8])).to_f32();
            if x(t1).lt(F80::ONE) {
                m.set_f32(q_out, (x(t1) * dx2 + x(v[6])).to_f32());
                m.set_f32(q_out + 4, (x(v[7]) + x(t1) * dy2).to_f32());
                m.set_f32(q_out + 8, (x(v[8]) + x(t1) * dz2).to_f32());
            }
            if x(t0).gt(F80::ZERO) {
                m.set_f32(p_out, (dx2 * t0 + x(v[6])).to_f32());
                m.set_f32(p_out + 4, (x(t0) * dy2 + x(v[7])).to_f32());
                m.set_f32(p_out + 8, (x(v[8]) + x(t0) * dz2).to_f32());
            }
        }
    }
    if res == 1 && x(t1).le(F80::ONE) && F80::ZERO.le(x(t0)) && x(t0).gt(x(0.5f64)) {
        for k in 0..3 {
            let w = m.u32(q_out + 4 * k);
            m.set_u32(p_out + 4 * k, w);
        }
    }
    res
}

/// Appends a point to the contact surface at `s`, with the model it belongs to
/// [soft 0x084a1f50 surfaceOfContactAddVertex]. The arrays are made with room for 8 points
/// when the first is added and grown by 8 (three `SI_realloc`s) when a multiple of 8 has
/// been reached and the capacity is used up. The point is stored as three words; the count
/// of changes of model along the list ([`soc::RUNS`]) goes up when the model differs from
/// the previous point's.
pub fn surface_of_contact_add_vertex(m: &mut Mem, vx: u32, vy: u32, vz: u32, model: u32, s: u32) {
    let n = m.u32(s + soc::COUNT);
    if n & 7 == 0 {
        if m.u32(s + soc::POINTS) == 0 {
            m.set_u32(s + soc::CAP, 8);
            let a = m.si_malloc(0x60);
            m.set_u32(s + soc::POINTS, a);
            let cap = m.u32(s + soc::CAP);
            let b = m.si_malloc(cap.wrapping_mul(12));
            m.set_u32(s + soc::SECOND, b);
            let c = m.si_malloc(cap.wrapping_mul(4));
            m.set_u32(s + soc::MODELS, c);
            m.set_u32(s + soc::FIELD_84, 0);
        } else if (m.i32(s + soc::CAP)) <= n as i32 {
            let cap = m.u32(s + soc::CAP).wrapping_add(8);
            m.set_u32(s + soc::CAP, cap);
            let a = m.si_realloc(m.u32(s + soc::POINTS), cap.wrapping_mul(12));
            m.set_u32(s + soc::POINTS, a);
            let cap = m.u32(s + soc::CAP);
            let b = m.si_realloc(m.u32(s + soc::SECOND), cap.wrapping_mul(12));
            m.set_u32(s + soc::SECOND, b);
            let cap = m.u32(s + soc::CAP);
            let c = m.si_realloc(m.u32(s + soc::MODELS), cap.wrapping_mul(4));
            m.set_u32(s + soc::MODELS, c);
        }
    }
    let n = m.u32(s + soc::COUNT);
    let pts = m.u32(s + soc::POINTS);
    let at = pts.wrapping_add(n.wrapping_mul(12));
    m.set_u32(at, vx);
    m.set_u32(at + 4, vy);
    m.set_u32(at + 8, vz);
    let ids = m.u32(s + soc::MODELS);
    m.set_u32(ids.wrapping_add(n.wrapping_mul(4)), model);
    let n = m.u32(s + soc::COUNT);
    if n == 0 {
        m.set_u32(s + soc::RUNS, 0);
    } else if m.u32(m.u32(s + soc::MODELS).wrapping_sub(4).wrapping_add(n.wrapping_mul(4))) != model {
        let r = m.u32(s + soc::RUNS).wrapping_add(1);
        m.set_u32(s + soc::RUNS, r);
    }
    let n = m.u32(s + soc::COUNT).wrapping_add(1);
    m.set_u32(s + soc::COUNT, n);
}

/// The area of the contact polygon: half the sum of the lengths of the cross products
/// of consecutive point triples, taken two at a time
/// [soft 0x084a206c surfaceOfContactComputeArea]. Fewer than 3 points give 0. The result
/// is the float the original leaves in st0.
pub fn surface_of_contact_compute_area(m: &Mem, s: u32) -> f32 {
    let mut sum = 0.0f32;
    let mut n = m.i32(s + soc::COUNT);
    if n > 2 {
        let pts = m.u32(s + soc::POINTS);
        let mut i: i32 = 0;
        while i < n {
            let a = pts.wrapping_add((((i + 2) % n) as u32).wrapping_mul(12));
            let b = pts.wrapping_add((((i + 1) % n) as u32).wrapping_mul(12));
            let c = pts.wrapping_add((i as u32).wrapping_mul(12));
            let d = |p: u32, q: u32, k: u32| lf(m, p + 4 * k) - lf(m, q + 4 * k);
            // the differences stay in registers; the stores to locals are never read back
            let (t1, t2, t3) = (d(a, b, 0), d(a, b, 1), d(a, b, 2));
            let (t4, t5, t6) = (d(c, b, 0), d(c, b, 1), d(c, b, 2));
            let cx = t2 * t6 - t3 * t5;
            let cy = t3 * t4 - t1 * t6;
            let cz = t1 * t5 - t4 * t2;
            sum = ((((cy * cy) + (cx * cx)) + (cz * cz)).sqrt() + x(sum)).to_f32();
            n = m.i32(s + soc::COUNT);
            i += 2;
        }
        sum = (x(0.5f64) * sum).to_f32();
    }
    sum
}

/// The sphere-sphere test [soft 0x084a217c checkBsphBsphCollision]: the centre of the
/// second sphere relative to the first is `c`, the radii `r1`, `r2`. When the distance
/// (rounded to a float) is below the sum of the radii, the contact point (the first sphere's
/// surface point towards the other, `out_pt`) and two angles (`out_ang[1]`, `out_ang[2]`:
/// `atan2(z, x)` and the negative of `atan2(x, y)` of the unit direction) are stored and 1
/// is returned. A distance not above 0.0001 leaves the unit direction as the stack held it
/// (the image's fill in a check).
pub fn check_bsph_bsph_collision(m: &mut Mem, c: [f32; 3], r1: f32, r2: f32, out_pt: u32, out_ang: u32) -> i16 {
    for k in 0..3 {
        m.set_u32(out_pt + 4 * k, 0);
        m.set_u32(out_ang + 4 * k, 0);
    }
    let loc = m.local(12);
    let s = (((x(c[1]) * c[1]) + (x(c[0]) * c[0])) + (x(c[2]) * c[2])).sqrt().to_f32();
    let mut res = 0;
    if x(s).lt(x(r2) + r1) {
        res = 1;
        if x(s).gt(x(0.0001f64)) {
            m.set_f32(loc, (x(c[0]) / x(s)).to_f32());
            m.set_f32(loc + 4, (x(c[1]) / x(s)).to_f32());
            m.set_f32(loc + 8, (x(c[2]) / x(s)).to_f32());
        }
        let neg = -x(r1);
        let (ux, uy, uz) = (m.f32(loc), m.f32(loc + 4), m.f32(loc + 8));
        m.set_f32(out_pt, (neg * ux).to_f32());
        m.set_f32(out_pt + 4, (neg * uy).to_f32());
        m.set_f32(out_pt + 8, (neg * uz).to_f32());
        m.set_f32(out_ang + 4, fpatan(x(uz), x(ux)).to_f32());
        let a = fpatan(x(ux), x(uy)).to_f32();
        m.set_f32(out_ang + 8, (-x(a)).to_f32());
    }
    m.release(12);
    res
}

/// The sphere-plane test [soft 0x084a2274 checkBsphBplnCollision]: a sphere of radius `r`
/// whose centre is at height `cy` against the plane at height `h` (the plane is `y = h`
/// in the box's frame). Stores the contact point `(0, -r, 0)` in `out_pt` (`out_ang` is
/// zeroed) and returns 1 when `cy - r <= h`.
pub fn check_bsph_bpln_collision(m: &mut Mem, cy: f32, r: f32, h: f32, out_pt: u32, out_ang: u32) -> i16 {
    for k in 0..3 {
        m.set_u32(out_pt + 4 * k, 0);
        m.set_u32(out_ang + 4 * k, 0);
    }
    m.set_f32(out_pt + 4, (-x(r)).to_f32());
    (x(cy) - r).le(x(h)) as i16
}

const NEG_HALF_PI: f32 = f32::from_bits(0xbfc9_0fdb);
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
const PI32: f32 = f32::from_bits(0x4049_0fdb);

/// The sphere-box test [soft 0x084a22d0 checkBsphBboxCollision]: a sphere of radius `r`
/// whose centre `c` is relative to the box with corners `lo` and `hi`. For each axis the
/// face the centre is on the near side of is found (`sign` = ±1, the face's coordinate); the
/// sphere touches the box face when it reaches the face and the other two coordinates are
/// within the box, otherwise the edge (two faces) or corner (three). `out_pt` gets the
/// contact point on the sphere, `out_ang` angles of the contact normal. Returns 1 on contact
/// (and 0 otherwise, with the output as far as it had been written).
pub fn check_bsph_bbox_collision(m: &mut Mem, c: [f32; 3], r: f32, lo: [f32; 3], hi: [f32; 3], out_pt: u32, out_ang: u32) -> i32 {
    for k in 0..3 {
        m.set_u32(out_pt + 4 * k, 0);
        m.set_u32(out_ang + 4 * k, 0);
    }
    let [cx, cy, cz] = c;
    let (xr, xc) = (x(r), [x(cx), x(cy), x(cz)]);
    let in_range = |v: f32, i: usize| x(v).le(x(hi[i])) && x(v).ge(x(lo[i]));
    let mut sign = [0.0f32; 3]; // L_1c, L_18, L_14
    let mut face = [0.0f32; 3]; // L_28, L_24, L_20
    let mut hit = [false; 3]; // L_8, L_6, L_4: the sphere reaches the face of the axis
    let mut inside = [false; 3]; // L_10, L_e, L_c: and the other two coordinates are in the box
    for axis in 0..3 {
        let v = xc[axis];
        let (near, s, f);
        if F80::ZERO.lt(v) {
            near = (v - xr).le(x(hi[axis]));
            s = 1.0f32;
            f = hi[axis];
        } else {
            near = x(lo[axis]).le(v + xr);
            s = -1.0f32;
            f = lo[axis];
        }
        if near {
            sign[axis] = s;
            face[axis] = f;
            hit[axis] = true;
            let (a, b) = match axis {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            if in_range(c[a], a) && in_range(c[b], b) {
                inside[axis] = true;
            }
        }
    }
    let [hx, hy, hz] = hit;
    let [ix, iy, iz] = inside;
    let pos = |v: f32| F80::ZERO.lt(x(v));
    if hx {
        if ix {
            let v = if pos(sign[0]) { -xr } else { xr };
            m.set_f32(out_pt, v.to_f32());
            m.set_f32(out_ang + 8, if pos(sign[0]) { NEG_HALF_PI } else { HALF_PI });
            return 1;
        }
    } else if !hy && !hz {
        return 0;
    }
    if hy && iy {
        let v = if pos(sign[1]) { -xr } else { xr };
        m.set_f32(out_pt + 4, v.to_f32());
        m.set_f32(out_ang + 8, if pos(sign[1]) { 0.0 } else { PI32 });
        return 1;
    }
    if hz && iz {
        let v = if pos(sign[2]) { -xr } else { xr };
        m.set_f32(out_pt + 8, v.to_f32());
        m.set_f32(out_ang, if pos(sign[2]) { HALF_PI } else { NEG_HALF_PI });
        return 1;
    }
    // the sphere is nearest to an edge or a corner: the vector from the corner point
    let d0 = xc[0] - x(face[0]);
    let d1 = xc[1] - x(face[1]);
    let d2 = xc[2] - x(face[2]);
    let (l34, l30, l2c) = (d0.to_f32(), d1.to_f32(), d2.to_f32());
    // the words L_34, L_30, L_2c the final normalisation reads
    let mut dx = l34;
    let mut dy = l30;
    let mut dz = l2c;
    // A: x and y faces, z within the box: a vertical edge
    let a_test = hx && hy && x(cz).ge(x(lo[2])) && x(cz).le(x(hi[2]));
    let b_test = !a_test && hy && hz && x(cx).ge(x(lo[0])) && x(cx).le(x(hi[0]));
    if a_test {
        dz = 0.0;
        let s = (((d0 * d0) + (d1 * d1)) + F80::ZERO).sqrt().to_f32();
        if x(s).gt(xr) {
            return 0;
        }
        let a = fpatan(d0, d1).to_f32();
        m.set_f32(out_ang + 8, (-x(a)).to_f32());
    } else if b_test {
        dx = 0.0;
        let (ey, ez) = (x(l30), x(l2c));
        let s = ((F80::ZERO + ey * ey) + ez * ez).sqrt().to_f32();
        if x(s).gt(xr) {
            return 0;
        }
        m.set_f32(out_ang, fpatan(ez, ey).to_f32());
    } else {
        if !hx {
            return 0;
        }
        let c_body = hz && x(cy).ge(x(lo[1])) && x(cy).le(x(hi[1]));
        if c_body {
            dy = 0.0;
            let (ex, ez) = (x(l34), x(l2c));
            let s = ((F80::ZERO + ex * ex) + ez * ez).sqrt().to_f32();
            if x(s).gt(xr) {
                return 0;
            }
            m.set_f32(out_ang, if pos(sign[2]) { HALF_PI } else { NEG_HALF_PI });
            let a = fpatan(ez, ex).to_f32();
            m.set_f32(out_ang + 8, (-x(a)).to_f32());
        } else {
            if !(hx && hy && hz) {
                return 0;
            }
            let (ex, ey, ez) = (x(l34), x(l30), x(l2c));
            let s = (((ey * ey) + (ex * ex)) + (ez * ez)).sqrt().to_f32();
            if x(s).gt(xr) {
                return 0;
            }
            let a = fpatan(ex, ey).to_f32();
            m.set_f32(out_ang + 8, (-x(a)).to_f32());
            let b = fpatan(ex, ez).to_f32();
            m.set_f32(out_ang + 4, (-x(b)).to_f32());
        }
    }
    m.set_f32(out_pt, dx);
    m.set_f32(out_pt + 4, dy);
    m.set_f32(out_pt + 8, dz);
    let (px, py, pz) = (lf(m, out_pt), lf(m, out_pt + 4), lf(m, out_pt + 8));
    let len = (((py * py) + (px * px)) + (pz * pz)).sqrt().to_f32();
    if x(len).abs().gt(x(0.0001f64)) {
        m.set_f32(out_pt, (px / x(len)).to_f32());
        m.set_f32(out_pt + 4, (py / x(len)).to_f32());
        m.set_f32(out_pt + 8, (pz / x(len)).to_f32());
    }
    let neg = -xr;
    let (qx, qy, qz) = (lf(m, out_pt), lf(m, out_pt + 4), lf(m, out_pt + 8));
    m.set_f32(out_pt, (qx * neg).to_f32());
    m.set_f32(out_pt + 4, (qy * neg).to_f32());
    m.set_f32(out_pt + 8, (neg * qz).to_f32());
    1
}

/// Offsets of the model record (`__SI_Model`, 0x430 bytes) this file reads; others are named
/// where a function uses them.
pub mod mdl {
    /// +0x10: the six floats of the bounding box (x0, y0, z0, x1, y1, z1 in the order the
    /// cube builder below reads them: 0x10, 0x14, 0x18 low, 0x1c, 0x20, 0x24 high).
    pub const BBOX: u32 = 0x10;
    /// +0x38: the geometry (definition) list.
    pub const DEFS: u32 = 0x38;
    /// +0x16c: the three scale factors of the shape (x, y, z).
    pub const SCALE: u32 = 0x16c;
    /// +0x178: the matrix of the model in its parent (4 x 4 floats).
    pub const LOCAL: u32 = 0x178;
    /// +0x1c4: the global matrix (4 x 4 floats).
    pub const GLOBAL: u32 = 0x1c4;
}

/// `n.260`, a short the grid size of the collision plane is read from (data, value 1).
const N_260: u32 = 0x086d_7280;

/// Sets up the stand-in model a collision test is made against [soft 0x084a53d4 model4BoolPrepare]: `*pp` is made once (a model with a sphere of radius 1 and 8 x 8
/// bands for `level` 0, a 10000-sized grid for -1, a unit cube about the origin for 1) and
/// its matrix is then taken from `model` (level 0: the local matrix with its three axes
/// scaled by the larger of the two entries of each axis row of `model`'s box and the scale;
/// -1: the global matrix and the plane offset in `vec`; 1: the global matrix and the corners
/// of the box of `model` written into the cube's vertices).
pub fn model4_bool_prepare(m: &mut Mem, o: &mut dyn Outside, pp: u32, model: u32, level: i32, vec: u32) {
    if m.u32(pp) == 0 {
        let nm = o.si_model_allocate(m, 0);
        m.set_u32(pp, nm);
        let geometry = match level {
            0 => Some(o.si_mesh_sphere(m, 1.0, 8, 8)),
            -1 => {
                let n = m.i16(N_260) as i32;
                Some(o.si_mesh_grid(m, n, n, 10000.0, 10000.0))
            }
            1 => Some(o.si_mesh_cube(m, 0.5, 0.5, 0.5, -0.5, -0.5, -0.5)),
            _ => None,
        };
        if let Some(g) = geometry {
            if g != 0 {
                let cur = m.u32(pp);
                o.si_model_add_definition(m, cur, 4, g);
            }
        }
    }
    let dst = m.u32(pp);
    match level {
        0 => {
            o.si_matrix_copy(m, model + mdl::LOCAL, dst + mdl::GLOBAL);
            // the larger magnitude of the two entries of each axis, times the scale
            let pick = |m: &Mem, hi: u32, lo: u32| {
                let (a, b) = (lf(m, model + hi).abs(), lf(m, model + lo).abs());
                if b.lt(a) { a } else { b }
            };
            let sx = (pick(m, 0x10, 0x1c) * lf(m, model + mdl::SCALE)).to_f32();
            let sy = (pick(m, 0x14, 0x20) * lf(m, model + mdl::SCALE + 4)).to_f32();
            let sz = (pick(m, 0x18, 0x24) * lf(m, model + mdl::SCALE + 8)).to_f32();
            for (row, s) in [(0u32, sx), (0x10, sy), (0x20, sz)] {
                for k in 0..3 {
                    let a = dst + mdl::GLOBAL + row + 4 * k;
                    let p = lf(m, a) * x(s);
                    m.set_f32(a, p.to_f32());
                }
            }
            for k in 0..3 {
                let v = lf(m, model + 0x1b8 + 4 * k).to_f32();
                m.set_f32(dst + 0x1f4 + 4 * k, v);
            }
        }
        -1 => {
            o.si_matrix_copy(m, model + mdl::GLOBAL, dst + mdl::GLOBAL);
            m.set_u32(vec, 0);
            m.set_u32(vec + 8, 0);
            let t1 = lf(m, model + 0x20) * lf(m, model + 0x170);
            m.set_f32(vec + 4, t1.to_f32());
            let zero = x(0.0f32);
            let l = |a: u32| lf(m, model + a);
            let t2 = (((l(0x178) * zero) + (t1 * l(0x188))) + (l(0x198) * zero)) + l(0x1a8);
            let t3 = ((((l(0x17c) * zero) + (t1 * l(0x18c))) + (l(0x19c) * zero)) + l(0x1ac)).to_f32();
            let t4 = (((zero * l(0x1a0)) + ((t1 * l(0x190)) + (l(0x180) * zero))) + l(0x1b0)).to_f32();
            let d0 = lf(m, dst + 0x1f4);
            m.set_f32(dst + 0x1f4, (t2 + d0).to_f32());
            let d1 = lf(m, dst + 0x1f8);
            m.set_f32(dst + 0x1f8, (d1 + x(t3)).to_f32());
            let d2 = lf(m, dst + 0x1fc);
            m.set_f32(dst + 0x1fc, (d2 + x(t4)).to_f32());
        }
        1 => {
            o.si_matrix_copy(m, model + mdl::GLOBAL, dst + mdl::GLOBAL);
            let defs = m.u32(dst + mdl::DEFS);
            let first = m.u32(defs);
            let v = m.u32(first);
            let b = |m: &Mem, k: u32| lf(m, model + 0x10 + 4 * k).to_f32();
            let (a0, a1, a2, a3, a4, a5) = (b(m, 0), b(m, 1), b(m, 2), b(m, 3), b(m, 4), b(m, 5));
            // corner k = (x from {0, 3}, y from {1, 4}, z from {2, 5}), vertices 0x14 apart
            let corners = [
                (a0, a1, a2),
                (a0, a1, a5),
                (a0, a4, a2),
                (a0, a4, a5),
                (a3, a1, a2),
                (a3, a1, a5),
                (a3, a4, a2),
                (a3, a4, a5),
            ];
            for (k, (cx, cy, cz)) in corners.iter().enumerate() {
                let p = v + 0x14 * k as u32;
                m.set_f32(p, *cx);
                m.set_f32(p + 4, *cy);
                m.set_f32(p + 8, *cz);
            }
        }
        _ => {}
    }
}

// Reads and writes of the image by a signed address, as the drafts of the larger routines use them.
fn rf(m: &Mem, a: i32) -> F80 {
    x(m.f32(a as u32))
}
fn ri(m: &Mem, a: i32) -> i32 {
    m.i32(a as u32)
}
fn rs(m: &Mem, a: i32) -> i32 {
    m.i16(a as u32) as i32
}

/// [soft 0x084a28b4 detectDynaBspherecollision]: the bounding-sphere test of two bodies of the
/// simulation; see the finding for what is read from the code and what is a guess.
#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case)]
pub fn detect_dyna_bsphere_collision(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, mdl3: u32, mdl4: u32, add_vertex: i16, soc: u32) -> i16 {
    let fr = m.local(0x7e);
    let r = detect_dyna_bsphere_collision_body(m, o, fr + 0x7e, mdl1 as i32, mdl2 as i32, mdl3 as i32, mdl4 as i32, add_vertex as i32, soc as i32);
    m.release(0x7e);
    r as i16
}

#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::too_many_arguments)]
fn detect_dyna_bsphere_collision_body(m: &mut Mem, o: &mut dyn Outside, fp: u32, mdl1: i32, mdl2: i32, mdl3: i32, mdl4: i32, add_vertex: i32, soc: i32) -> i32 {
    let fp = fp as i32;
    let mut fVar1 = x(0.0f32);
    let mut fVar2 = x(0.0f32);
    let mut fVar3 = x(0.0f32);
    let mut level: i32 = 0;
    let mut body3: i32 = 0;
    let mut iVar6: i32 = 0;
    let mut iVar7: i32 = 0;
    let mut iVar8: i32 = 0;
    let mut iVar9: i32 = 0;
    let mut fVar10 = x(0.0f32);
    let mut fVar11 = x(0.0f32);
    let mut radius_of: i32 = 0;
    m.set_i16(((fp - 0x7e)) as u32, (0) as i16);
    body3 = ri(m, (mdl3 + 0x300));
    radius_of = mdl2;
    if mdl2 == 0 {
        radius_of = mdl3;
    }
    m.set_i32(((fp - 0x78)) as u32, mdl2);
    if (mdl2 == 0) || (ri(m, (mdl2 + 0x2f4)) == 0) {
        m.set_i32(((fp - 0x78)) as u32, mdl3);
    }
    m.set_f32(((fp - 0x14)) as u32, x(0.0f32).to_f32());
    m.set_f32(((fp - 0x18)) as u32, x(0.0f32).to_f32());
    m.set_f32(((fp - 0x1c)) as u32, x(0.0f32).to_f32());
    if ri(m, (mdl1 + 0x2ec)) == 2 {
        if mdl2 != 0 {
            iVar6 = ri(m, (mdl3 + 0x2c4));
            if mdl2 != ri(m, (iVar6 + 0x2c0)) {
                fVar2 = (((((((rf(m, (mdl2 + 0x188)) * x(0.0f32)) + (rf(m, (mdl2 + 0x178)) * x(0.0f32))) + (rf(m, (mdl2 + 0x198)) * x(0.0f32))) + rf(m, (mdl2 + 0x1a8))) + rf(m, (mdl2 + 0x1b8)))) - rf(m, (ri(m, (iVar6 + 0x2c0)) + 0x1b8)));
                fVar1 = (((((((rf(m, (mdl2 + 0x18c)) * x(0.0f32)) + (rf(m, (mdl2 + 0x17c)) * x(0.0f32))) + (rf(m, (mdl2 + 0x19c)) * x(0.0f32))) + rf(m, (mdl2 + 0x1ac))) + rf(m, (mdl2 + 0x1bc)))) - rf(m, (ri(m, (iVar6 + 0x2c0)) + 0x1bc)));
                fVar10 = (((((((rf(m, (mdl2 + 0x190)) * x(0.0f32)) + (rf(m, (mdl2 + 0x180)) * x(0.0f32))) + (rf(m, (mdl2 + 0x1a0)) * x(0.0f32))) + rf(m, (mdl2 + 0x1b0))) + rf(m, (mdl2 + 0x1c0)))) - rf(m, (ri(m, (iVar6 + 0x2c0)) + 0x1c0)));
                iVar7 = ri(m, (iVar6 + 0x2c0));
                m.set_f32(((fp - 0x1c)) as u32, (((rf(m, (iVar7 + 0x180)) * fVar10) + (rf(m, (iVar7 + 0x17c)) * fVar1)) + (rf(m, (iVar7 + 0x178)) * fVar2)).to_f32());
                iVar7 = ri(m, (iVar6 + 0x2c0));
                m.set_f32(((fp - 0x18)) as u32, (((rf(m, (iVar7 + 0x190)) * fVar10) + (rf(m, (iVar7 + 0x18c)) * fVar1)) + (rf(m, (iVar7 + 0x188)) * fVar2)).to_f32());
                iVar6 = ri(m, (iVar6 + 0x2c0));
                m.set_f32(((fp - 0x14)) as u32, (((fVar2 * rf(m, (iVar6 + 0x198))) + (fVar1 * rf(m, (iVar6 + 0x19c)))) + (fVar10 * rf(m, (iVar6 + 0x1a0)))).to_f32());
            }
            iVar6 = ri(m, (mdl3 + 0x2c4));
            iVar7 = ri(m, (iVar6 + 0x2c0));
            iVar8 = ri(m, (iVar6 + 0x2c0));
            fVar1 = (rf(m, (fp - 0x1c)) * rf(m, (iVar8 + 0x11c)));
            iVar9 = ri(m, (iVar6 + 0x2c0));
            fVar10 = (rf(m, (fp - 0x1c)) * rf(m, (iVar9 + 0x120)));
            fVar2 = (rf(m, (fp - 0x18)) * rf(m, (iVar9 + 0x130)));
            m.set_f32(((fp - 0x1c)) as u32, (((((rf(m, (fp - 0x14)) * rf(m, (iVar7 + 0x138))) + (rf(m, (fp - 0x1c)) * rf(m, (iVar7 + 0x118)))) + (rf(m, (fp - 0x18)) * rf(m, (iVar7 + 0x128)))) + rf(m, (iVar7 + 0x148))) + rf(m, (ri(m, (iVar6 + 0x2c0)) + 0x158))).to_f32());
            m.set_f32(((fp - 0x18)) as u32, (((((rf(m, (fp - 0x14)) * rf(m, (iVar8 + 0x13c))) + (rf(m, (fp - 0x18)) * rf(m, (iVar8 + 0x12c)))) + fVar1) + rf(m, (iVar8 + 0x14c))) + rf(m, (ri(m, (iVar6 + 0x2c0)) + 0x15c))).to_f32());
            m.set_f32(((fp - 0x14)) as u32, ((((fVar10 + fVar2) + (rf(m, (fp - 0x14)) * rf(m, (iVar9 + 0x140)))) + rf(m, (iVar9 + 0x150))) + rf(m, (ri(m, (iVar6 + 0x2c0)) + 0x160))).to_f32());
        }
        m.set_f32(((fp - 0x10)) as u32, ((((rf(m, (body3 + 0x1e0)) * rf(m, (fp - 0x14))) + (rf(m, (body3 + 0x1d8)) * rf(m, (fp - 0x1c)))) + (rf(m, (body3 + 0x1dc)) * rf(m, (fp - 0x18)))) + rf(m, (body3 + 0x244))).to_f32());
        m.set_f32(((fp - 0xc)) as u32, ((((rf(m, (body3 + 0x1ec)) * rf(m, (fp - 0x14))) + (rf(m, (body3 + 0x1e8)) * rf(m, (fp - 0x18)))) + (rf(m, (body3 + 0x1e4)) * rf(m, (fp - 0x1c)))) + rf(m, (body3 + 0x248))).to_f32());
        m.set_f32(((fp - 0x8)) as u32, ((((rf(m, (fp - 0x1c)) * rf(m, (body3 + 0x1f0))) + (rf(m, (fp - 0x18)) * rf(m, (body3 + 0x1f4)))) + (rf(m, (fp - 0x14)) * rf(m, (body3 + 0x1f8)))) + rf(m, (body3 + 0x24c))).to_f32());
    } else {
        m.set_f32(((fp - 0x10)) as u32, x(0.0f32).to_f32());
        m.set_f32(((fp - 0xc)) as u32, x(0.0f32).to_f32());
        m.set_f32(((fp - 0x8)) as u32, x(0.0f32).to_f32());
        if mdl2 != mdl1 {
            fVar2 = (((((((rf(m, (mdl2 + 0x188)) * x(0.0f32)) + (rf(m, (mdl2 + 0x178)) * x(0.0f32))) + (rf(m, (mdl2 + 0x198)) * x(0.0f32))) + rf(m, (mdl2 + 0x1a8))) + rf(m, (mdl2 + 0x1b8)))) - rf(m, (mdl1 + 0x1b8)));
            fVar1 = (((((((rf(m, (mdl2 + 0x18c)) * x(0.0f32)) + (rf(m, (mdl2 + 0x17c)) * x(0.0f32))) + (rf(m, (mdl2 + 0x19c)) * x(0.0f32))) + rf(m, (mdl2 + 0x1ac))) + rf(m, (mdl2 + 0x1bc)))) - rf(m, (mdl1 + 0x1bc)));
            fVar10 = (((((((rf(m, (mdl2 + 0x190)) * x(0.0f32)) + (rf(m, (mdl2 + 0x180)) * x(0.0f32))) + (rf(m, (mdl2 + 0x1a0)) * x(0.0f32))) + rf(m, (mdl2 + 0x1b0))) + rf(m, (mdl2 + 0x1c0)))) - rf(m, (mdl1 + 0x1c0)));
            m.set_f32(((fp - 0x10)) as u32, (((rf(m, (mdl1 + 0x180)) * fVar10) + (rf(m, (mdl1 + 0x178)) * fVar2)) + (rf(m, (mdl1 + 0x17c)) * fVar1)).to_f32());
            m.set_f32(((fp - 0xc)) as u32, (((rf(m, (mdl1 + 0x190)) * fVar10) + (rf(m, (mdl1 + 0x18c)) * fVar1)) + (rf(m, (mdl1 + 0x188)) * fVar2)).to_f32());
            m.set_f32(((fp - 0x8)) as u32, (((fVar10 * rf(m, (mdl1 + 0x1a0))) + (fVar2 * rf(m, (mdl1 + 0x198)))) + (fVar1 * rf(m, (mdl1 + 0x19c)))).to_f32());
        }
    }
    fVar2 = (((((((rf(m, (fp - 0x10)) * rf(m, (mdl1 + 0x178))) + (rf(m, (fp - 0xc)) * rf(m, (mdl1 + 0x188)))) + (rf(m, (fp - 0x8)) * rf(m, (mdl1 + 0x198)))) + rf(m, (mdl1 + 0x1a8))) + rf(m, (mdl1 + 0x1b8)))) - rf(m, (mdl4 + 0x1b8)));
    fVar1 = (((((((rf(m, (fp - 0xc)) * rf(m, (mdl1 + 0x18c))) + (rf(m, (fp - 0x10)) * rf(m, (mdl1 + 0x17c)))) + (rf(m, (fp - 0x8)) * rf(m, (mdl1 + 0x19c)))) + rf(m, (mdl1 + 0x1ac))) + rf(m, (mdl1 + 0x1bc)))) - rf(m, (mdl4 + 0x1bc)));
    fVar10 = (((((((rf(m, (fp - 0x10)) * rf(m, (mdl1 + 0x180))) + (rf(m, (fp - 0xc)) * rf(m, (mdl1 + 0x190)))) + (rf(m, (fp - 0x8)) * rf(m, (mdl1 + 0x1a0)))) + rf(m, (mdl1 + 0x1b0))) + rf(m, (mdl1 + 0x1c0)))) - rf(m, (mdl4 + 0x1c0)));
    m.set_f32(((fp - 0x10)) as u32, (((rf(m, (mdl4 + 0x180)) * fVar10) + (rf(m, (mdl4 + 0x178)) * fVar2)) + (rf(m, (mdl4 + 0x17c)) * fVar1)).to_f32());
    m.set_f32(((fp - 0xc)) as u32, (((rf(m, (mdl4 + 0x190)) * fVar10) + (rf(m, (mdl4 + 0x18c)) * fVar1)) + (rf(m, (mdl4 + 0x188)) * fVar2)).to_f32());
    m.set_f32(((fp - 0x8)) as u32, (((fVar10 * rf(m, (mdl4 + 0x1a0))) + (fVar2 * rf(m, (mdl4 + 0x198)))) + (fVar1 * rf(m, (mdl4 + 0x19c)))).to_f32());
    fVar10 = x(0.0f32);
    if x(0.0f32).lt(rf(m, (radius_of + 0x16c)).abs()) {
        fVar10 = rf(m, (radius_of + 0x16c)).abs();
    }
    if fVar10.lt(rf(m, (radius_of + 0x170)).abs()) {
        fVar10 = rf(m, (radius_of + 0x170)).abs();
    }
    if fVar10.lt(rf(m, (radius_of + 0x174)).abs()) {
        fVar10 = rf(m, (radius_of + 0x174)).abs();
    }
    fVar1 = rf(m, (mdl4 + 0x16c));
    m.set_f32(((fp - 0x40)) as u32, (fVar1 * rf(m, (mdl4 + 0x10))).to_f32());
    fVar2 = rf(m, (mdl4 + 0x170));
    m.set_f32(((fp - 0x3c)) as u32, (fVar2 * rf(m, (mdl4 + 0x14))).to_f32());
    fVar3 = rf(m, (mdl4 + 0x174));
    m.set_f32(((fp - 0x38)) as u32, (fVar3 * rf(m, (mdl4 + 0x18))).to_f32());
    m.set_f32(((fp - 0x4c)) as u32, (fVar1 * rf(m, (mdl4 + 0x1c))).to_f32());
    m.set_f32(((fp - 0x48)) as u32, (fVar2 * rf(m, (mdl4 + 0x20))).to_f32());
    m.set_f32(((fp - 0x44)) as u32, (fVar3 * rf(m, (mdl4 + 0x24))).to_f32());
    fVar10 = (rf(m, (radius_of + 0x2c)) * fVar10);
    level = (rs(m, (soc + 0x78))) as i16 as i32;
    if level == 0 {
        fVar1 = fVar1.abs();
        if fVar1.le(x(0.0f32)) {
            fVar1 = x(0.0f32);
        }
        if fVar1.lt(fVar2.abs()) {
            fVar1 = fVar2.abs();
        }
        if fVar1.lt(fVar3.abs()) {
            fVar1 = fVar3.abs();
        }
        let res = (check_bsph_bsph_collision(m, [rf(m, (fp - 0x10)).to_f32(), rf(m, (fp - 0xc)).to_f32(), rf(m, (fp - 0x8)).to_f32()], fVar10.to_f32(), (rf(m, (mdl4 + 0x2c)) * fVar1).to_f32(), ((fp - 0x34)) as u32, ((fp - 0x58)) as u32)) as i16;
 m.set_i16((fp - 0x7e) as u32, res);
    } else if level < 1 {
        if level == (-1) {
            let res = check_bsph_bpln_collision(m, rf(m, (fp - 0xc)).to_f32(), fVar10.to_f32(), rf(m, (fp - 0x48)).to_f32(), (fp - 0x34) as u32, (fp - 0x58) as u32) as i16;
 m.set_i16((fp - 0x7e) as u32, res);
        }
    } else if level < 3 {
        let res = check_bsph_bbox_collision(m, [rf(m, (fp - 0x10)).to_f32(), rf(m, (fp - 0xc)).to_f32(), rf(m, (fp - 0x8)).to_f32()], fVar10.to_f32(), [rf(m, (fp - 0x40)).to_f32(), rf(m, (fp - 0x3c)).to_f32(), rf(m, (fp - 0x38)).to_f32()], [rf(m, (fp - 0x4c)).to_f32(), rf(m, (fp - 0x48)).to_f32(), rf(m, (fp - 0x44)).to_f32()], (fp - 0x34) as u32, (fp - 0x58) as u32) as i16;
 m.set_i16((fp - 0x7e) as u32, res);
    }
    if rs(m, (fp - 0x7e)) == 0 {
        return 0;
    }
    if ri(m, (mdl1 + 0x2ec)) == 2 {
        fVar1 = ((((rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x178))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x188)))) + (rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x198)))) + rf(m, (mdl4 + 0x1a8)));
        fVar2 = ((((rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x18c))) + (rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x17c)))) + (rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x19c)))) + rf(m, (mdl4 + 0x1ac)));
        fVar10 = ((((rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x180))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x190)))) + (rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x1a0)))) + rf(m, (mdl4 + 0x1b0)));
        m.set_f32(((fp - 0x28)) as u32, (((rf(m, (mdl1 + 0x180)) * fVar10) + (rf(m, (mdl1 + 0x178)) * fVar1)) + (rf(m, (mdl1 + 0x17c)) * fVar2)).to_f32());
        m.set_f32(((fp - 0x24)) as u32, (((rf(m, (mdl1 + 0x190)) * fVar10) + (rf(m, (mdl1 + 0x18c)) * fVar2)) + (rf(m, (mdl1 + 0x188)) * fVar1)).to_f32());
        fVar1 = (fVar1 * rf(m, (mdl1 + 0x198)));
        fVar2 = (fVar2 * rf(m, (mdl1 + 0x19c)));
        fVar10 = (fVar10 * rf(m, (mdl1 + 0x1a0)));
        m.set_f32(((fp - 0x20)) as u32, ((fVar1 + fVar2) + fVar10).to_f32());
    } else if ri(m, (mdl1 + 0x2ec)) == 12 {
        iVar6 = ri(m, (mdl1 + 0x2bc));
        if iVar6 != 0 {
            fVar1 = ((((rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x198))) + (rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x178)))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x188)))) + rf(m, (mdl4 + 0x1a8)));
            fVar2 = ((((rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x19c))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x18c)))) + (rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x17c)))) + rf(m, (mdl4 + 0x1ac)));
            fVar10 = ((((rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x180))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x190)))) + (rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x1a0)))) + rf(m, (mdl4 + 0x1b0)));
            m.set_f32(((fp - 0x28)) as u32, (((rf(m, (iVar6 + 0x180)) * fVar10) + (rf(m, (iVar6 + 0x17c)) * fVar2)) + (rf(m, (iVar6 + 0x178)) * fVar1)).to_f32());
            m.set_f32(((fp - 0x24)) as u32, (((rf(m, (iVar6 + 0x190)) * fVar10) + (rf(m, (iVar6 + 0x18c)) * fVar2)) + (rf(m, (iVar6 + 0x188)) * fVar1)).to_f32());
            fVar1 = (fVar1 * rf(m, (iVar6 + 0x198)));
            fVar2 = (fVar2 * rf(m, (iVar6 + 0x19c)));
            fVar10 = (fVar10 * rf(m, (iVar6 + 0x1a0)));
            m.set_f32(((fp - 0x20)) as u32, ((fVar1 + fVar2) + fVar10).to_f32());
        } else {
        m.set_f32(((fp - 0x28)) as u32, ((((rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x198))) + (rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x178)))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x188)))) + rf(m, (mdl4 + 0x1a8))).to_f32());
        m.set_f32(((fp - 0x24)) as u32, ((((rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x19c))) + (rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x17c)))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x18c)))) + rf(m, (mdl4 + 0x1ac))).to_f32());
        m.set_f32(((fp - 0x20)) as u32, ((((rf(m, (fp - 0x2c)) * rf(m, (mdl4 + 0x1a0))) + (rf(m, (fp - 0x34)) * rf(m, (mdl4 + 0x180)))) + (rf(m, (fp - 0x30)) * rf(m, (mdl4 + 0x190)))) + rf(m, (mdl4 + 0x1b0))).to_f32());
        }
    }
    fVar10 = (rf(m, (body3 + 0x22c)) * rf(m, (fp - 0x28)));
    fVar1 = (rf(m, (fp - 0x28)) * rf(m, (body3 + 0x238)));
    fVar2 = (rf(m, (fp - 0x24)) * rf(m, (body3 + 0x23c)));
    // x87 order of the original (084a3308..): (A220 x + A224 y) + A228 z, (A22c x + A230 y) + A234 z; each plus the offset
    m.set_f32(((fp - 0x28)) as u32, ((((rf(m, (body3 + 0x220)) * rf(m, (fp - 0x28))) + (rf(m, (body3 + 0x224)) * rf(m, (fp - 0x24)))) + (rf(m, (body3 + 0x228)) * rf(m, (fp - 0x20)))) + rf(m, (fp - 0x1c))).to_f32());
    m.set_f32(((fp - 0x24)) as u32, ((((fVar10 + (rf(m, (body3 + 0x230)) * rf(m, (fp - 0x24)))) + (rf(m, (body3 + 0x234)) * rf(m, (fp - 0x20))))) + rf(m, (fp - 0x18))).to_f32());
    m.set_f32(((fp - 0x20)) as u32, (((fVar1 + fVar2) + (rf(m, (fp - 0x20)) * rf(m, (body3 + 0x240)))) + rf(m, (fp - 0x14))).to_f32());
    crate::dynamic::si_vector_clean(m, ((fp - 0x28)) as u32, 0);
    if (ri(m, (mdl1 + 0x2ec)) == 12) && (mdl2 != mdl1) {
        fVar2 = (((((((rf(m, (fp - 0x28)) * rf(m, (mdl2 + 0x178))) + (rf(m, (fp - 0x24)) * rf(m, (mdl2 + 0x188)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl2 + 0x198)))) + rf(m, (mdl2 + 0x1a8))) + rf(m, (mdl2 + 0x1b8)))) - rf(m, (mdl1 + 0x1b8)));
        fVar1 = (((((((rf(m, (fp - 0x24)) * rf(m, (mdl2 + 0x18c))) + (rf(m, (fp - 0x28)) * rf(m, (mdl2 + 0x17c)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl2 + 0x19c)))) + rf(m, (mdl2 + 0x1ac))) + rf(m, (mdl2 + 0x1bc)))) - rf(m, (mdl1 + 0x1bc)));
        fVar10 = (((((((rf(m, (fp - 0x28)) * rf(m, (mdl2 + 0x180))) + (rf(m, (fp - 0x24)) * rf(m, (mdl2 + 0x190)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl2 + 0x1a0)))) + rf(m, (mdl2 + 0x1b0))) + rf(m, (mdl2 + 0x1c0)))) - rf(m, (mdl1 + 0x1c0)));
        m.set_f32(((fp - 0x28)) as u32, (((rf(m, (mdl1 + 0x180)) * fVar10) + (rf(m, (mdl1 + 0x178)) * fVar2)) + (rf(m, (mdl1 + 0x17c)) * fVar1)).to_f32());
        m.set_f32(((fp - 0x24)) as u32, (((rf(m, (mdl1 + 0x190)) * fVar10) + (rf(m, (mdl1 + 0x18c)) * fVar1)) + (rf(m, (mdl1 + 0x188)) * fVar2)).to_f32());
        m.set_f32(((fp - 0x20)) as u32, (((fVar10 * rf(m, (mdl1 + 0x1a0))) + (fVar2 * rf(m, (mdl1 + 0x198)))) + (fVar1 * rf(m, (mdl1 + 0x19c)))).to_f32());
    }
    if add_vertex != 0 {
        surface_of_contact_add_vertex(m, rf(m, (fp - 0x28)).to_f32().to_bits(), rf(m, (fp - 0x24)).to_f32().to_bits(), rf(m, (fp - 0x20)).to_f32().to_bits(), ri(m, fp - 0x78) as u32, soc as u32);
    }
    m.set_f32(((soc + 0x88)) as u32, rf(m, (fp - 0x28)).to_f32());
    m.set_f32(((soc + 0x8c)) as u32, rf(m, (fp - 0x24)).to_f32());
    m.set_f32(((soc + 0x90)) as u32, rf(m, (fp - 0x20)).to_f32());
    m.set_f32(((soc + 0x10)) as u32, rf(m, (fp - 0x34)).to_f32());
    m.set_f32(((soc + 0x14)) as u32, rf(m, (fp - 0x30)).to_f32());
    m.set_i32(((soc + 0x74)) as u32, 0);
    m.set_f32(((soc + 0x18)) as u32, rf(m, (fp - 0x2c)).to_f32());
    m.set_f32(((soc + 0x1c)) as u32, rf(m, (fp - 0x34)).to_f32());
    m.set_f32(((soc + 0x20)) as u32, rf(m, (fp - 0x30)).to_f32());
    m.set_f32(((soc + 0x24)) as u32, rf(m, (fp - 0x2c)).to_f32());
    fVar10 = (((rf(m, (body3 + 0x1e0)) * rf(m, (fp - 0x20))) + (rf(m, (body3 + 0x1d8)) * rf(m, (fp - 0x28)))) + (rf(m, (body3 + 0x1dc)) * rf(m, (fp - 0x24))));
    fVar1 = (((rf(m, (body3 + 0x1ec)) * rf(m, (fp - 0x20))) + (rf(m, (body3 + 0x1e8)) * rf(m, (fp - 0x24)))) + (rf(m, (body3 + 0x1e4)) * rf(m, (fp - 0x28))));
    m.set_f32(((fp - 0x20)) as u32, (((rf(m, (body3 + 0x1f0)) * rf(m, (fp - 0x28))) + (rf(m, (fp - 0x24)) * rf(m, (body3 + 0x1f4)))) + (rf(m, (fp - 0x20)) * rf(m, (body3 + 0x1f8)))).to_f32());
    if ri(m, (mdl1 + 0x2ec)) == 2 {
        fVar11 = ((((rf(m, (fp - 0x20)) * rf(m, (mdl1 + 0x198))) + (fVar1 * rf(m, (mdl1 + 0x188)))) + (fVar10 * rf(m, (mdl1 + 0x178)))) + rf(m, (mdl1 + 0x1a8)));
        m.set_f32(((soc + 0x28)) as u32, fVar11.to_f32());
        fVar3 = ((((rf(m, (fp - 0x20)) * rf(m, (mdl1 + 0x19c))) + (fVar1 * rf(m, (mdl1 + 0x18c)))) + (fVar10 * rf(m, (mdl1 + 0x17c)))) + rf(m, (mdl1 + 0x1ac)));
        m.set_f32(((soc + 0x2c)) as u32, fVar3.to_f32());
        fVar2 = ((((fVar10 * rf(m, (mdl1 + 0x180))) + (fVar1 * rf(m, (mdl1 + 0x190)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl1 + 0x1a0)))) + rf(m, (mdl1 + 0x1b0)));
        m.set_f32(((soc + 0x30)) as u32, fVar2.to_f32());
        m.set_f32(((soc + 0x28)) as u32, (((rf(m, (mdl4 + 0x180)) * fVar2) + (rf(m, (mdl4 + 0x17c)) * fVar3)) + (rf(m, (mdl4 + 0x178)) * fVar11)).to_f32());
        m.set_f32(((soc + 0x2c)) as u32, (((rf(m, (mdl4 + 0x190)) * fVar2) + (rf(m, (mdl4 + 0x18c)) * fVar3)) + (rf(m, (mdl4 + 0x188)) * fVar11)).to_f32());
        m.set_f32(((soc + 0x30)) as u32, (((fVar11 * rf(m, (mdl4 + 0x198))) + (fVar3 * rf(m, (mdl4 + 0x19c)))) + (fVar2 * rf(m, (mdl4 + 0x1a0)))).to_f32());
    } else if ri(m, (mdl1 + 0x2ec)) == 12 {
        body3 = ri(m, (mdl1 + 0x2bc));
        if body3 == 0 {
            m.set_f32(((soc + 0x28)) as u32, (((rf(m, (mdl4 + 0x180)) * rf(m, (fp - 0x20))) + (rf(m, (mdl4 + 0x17c)) * fVar1)) + (rf(m, (mdl4 + 0x178)) * fVar10)).to_f32());
            m.set_f32(((soc + 0x2c)) as u32, (((rf(m, (mdl4 + 0x190)) * rf(m, (fp - 0x20))) + (rf(m, (mdl4 + 0x18c)) * fVar1)) + (rf(m, (mdl4 + 0x188)) * fVar10)).to_f32());
            m.set_f32(((soc + 0x30)) as u32, (((rf(m, (mdl4 + 0x198)) * fVar10) + (rf(m, (mdl4 + 0x19c)) * fVar1)) + (rf(m, (mdl4 + 0x1a0)) * rf(m, (fp - 0x20)))).to_f32());
        } else {
            fVar11 = ((((rf(m, (fp - 0x20)) * rf(m, (body3 + 0x198))) + (fVar1 * rf(m, (body3 + 0x188)))) + (fVar10 * rf(m, (body3 + 0x178)))) + rf(m, (body3 + 0x1a8)));
            m.set_f32(((soc + 0x28)) as u32, fVar11.to_f32());
            body3 = ri(m, (mdl1 + 0x2bc));
            fVar3 = ((((rf(m, (fp - 0x20)) * rf(m, (body3 + 0x19c))) + (fVar1 * rf(m, (body3 + 0x18c)))) + (fVar10 * rf(m, (body3 + 0x17c)))) + rf(m, (body3 + 0x1ac)));
            m.set_f32(((soc + 0x2c)) as u32, fVar3.to_f32());
            body3 = ri(m, (mdl1 + 0x2bc));
            fVar2 = ((((fVar10 * rf(m, (body3 + 0x180))) + (fVar1 * rf(m, (body3 + 0x190)))) + (rf(m, (fp - 0x20)) * rf(m, (body3 + 0x1a0)))) + rf(m, (body3 + 0x1b0)));
            m.set_f32(((soc + 0x30)) as u32, fVar2.to_f32());
            m.set_f32(((soc + 0x28)) as u32, (((rf(m, (mdl4 + 0x180)) * fVar2) + (rf(m, (mdl4 + 0x17c)) * fVar3)) + (rf(m, (mdl4 + 0x178)) * fVar11)).to_f32());
            m.set_f32(((soc + 0x2c)) as u32, (((rf(m, (mdl4 + 0x190)) * fVar2) + (rf(m, (mdl4 + 0x18c)) * fVar3)) + (rf(m, (mdl4 + 0x188)) * fVar11)).to_f32());
            m.set_f32(((soc + 0x30)) as u32, (((fVar11 * rf(m, (mdl4 + 0x198))) + (fVar3 * rf(m, (mdl4 + 0x19c)))) + (fVar2 * rf(m, (mdl4 + 0x1a0)))).to_f32());
        }
    }
    m.set_f32(((fp - 0x28)) as u32, fVar10.to_f32());
    m.set_f32(((fp - 0x24)) as u32, fVar1.to_f32());
    o.si_matrix_xyz_rotation(m, ((fp - 0x58)) as u32, ((soc + 0x34)) as u32);
    rs(m, fp - 0x7e)
}

/// [soft 0x084a3868 detectDynaBboxBsphCollision]: the box-against-sphere test of two bodies of the
/// simulation; see the finding for what is read from the code and what is a guess.

#[allow(clippy::too_many_arguments)]
pub fn detect_dyna_bbox_bsph_collision(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, mdl3: u32, mdl4: u32, add_vertex: i16, soc: u32) -> i16 {
    let fr = m.local(0xbc);
    let r = detect_dyna_bbox_bsph_collision_body(m, o, fr + 0xbc, mdl1 as i32, mdl2 as i32, mdl3 as i32, mdl4 as i32, add_vertex as i32, soc as i32);
    m.release(0xbc);
    r as i16
}

#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::too_many_arguments)]
fn detect_dyna_bbox_bsph_collision_body(m: &mut Mem, o: &mut dyn Outside, fp: u32, mdl1: i32, mdl2: i32, mdl3: i32, mdl4: i32, add_vertex: i32, soc: i32) -> i32 {
    let fp = fp as i32;
    let mut iVar1: i32 = 0;
    let mut iVar2: i32 = 0;
    let mut p_Var3: i32 = 0;
    let mut iVar4: i32 = 0;
    let mut fVar5 = x(0.0f32);
    let mut fVar6 = x(0.0f32);
    let mut fVar7 = x(0.0f32);
    let mut fVar8 = x(0.0f32);
    let mut fVar9 = x(0.0f32);
    let mut sVar10: i32 = 0;
    let mut lVar11 = x(0.0f32);
    iVar1 = ri(m, (mdl3 + 0x300));
    m.set_i32(((fp - 0xbc)) as u32, mdl2);
    if mdl2 == 0 {
        m.set_i32(((fp - 0xbc)) as u32, mdl3);
    }
    m.set_i32(((fp - 0x8c)) as u32, mdl2);
    if (mdl2 == 0) || (ri(m, (mdl2 + 0x2f4)) == 0) {
        m.set_i32(((fp - 0x8c)) as u32, mdl3);
    }
    fVar5 = (((((((rf(m, (mdl4 + 0x188)) * x(0.0f32)) + (rf(m, (mdl4 + 0x178)) * x(0.0f32))) + (rf(m, (mdl4 + 0x198)) * x(0.0f32))) + rf(m, (mdl4 + 0x1a8))) + rf(m, (mdl4 + 0x1b8)))) - rf(m, (mdl1 + 0x1b8)));
    fVar7 = (((((((rf(m, (mdl4 + 0x18c)) * x(0.0f32)) + (rf(m, (mdl4 + 0x17c)) * x(0.0f32))) + (rf(m, (mdl4 + 0x19c)) * x(0.0f32))) + rf(m, (mdl4 + 0x1ac))) + rf(m, (mdl4 + 0x1bc)))) - rf(m, (mdl1 + 0x1bc)));
    fVar6 = (((((((rf(m, (mdl4 + 0x180)) * x(0.0f32)) + (rf(m, (mdl4 + 0x190)) * x(0.0f32))) + (rf(m, (mdl4 + 0x1a0)) * x(0.0f32))) + rf(m, (mdl4 + 0x1b0))) + rf(m, (mdl4 + 0x1c0)))) - rf(m, (mdl1 + 0x1c0)));
    m.set_f32(((fp - 0x10)) as u32, (((rf(m, (mdl1 + 0x180)) * fVar6) + (rf(m, (mdl1 + 0x17c)) * fVar7)) + (rf(m, (mdl1 + 0x178)) * fVar5)).to_f32());
    m.set_f32(((fp - 0xc)) as u32, (((rf(m, (mdl1 + 0x190)) * fVar6) + (rf(m, (mdl1 + 0x188)) * fVar5)) + (rf(m, (mdl1 + 0x18c)) * fVar7)).to_f32());
    m.set_f32(((fp - 0x8)) as u32, (((rf(m, (mdl1 + 0x198)) * fVar5) + (rf(m, (mdl1 + 0x19c)) * fVar7)) + (rf(m, (mdl1 + 0x1a0)) * fVar6)).to_f32());
    'lab: {
    if ri(m, (mdl1 + 0x2ec)) == 2 {
        fVar5 = (rf(m, (fp - 0x10)) - rf(m, (iVar1 + 0x244)));
        fVar6 = (rf(m, (fp - 0xc)) - rf(m, (iVar1 + 0x248)));
        m.set_f32(((fp - 0x8)) as u32, (rf(m, (fp - 0x8)) - rf(m, (iVar1 + 0x24c))).to_f32());
        m.set_f32(((fp - 0x10)) as u32, (((rf(m, (iVar1 + 0x228)) * rf(m, (fp - 0x8))) + (rf(m, (iVar1 + 0x224)) * fVar6)) + (rf(m, (iVar1 + 0x220)) * fVar5)).to_f32());
        m.set_f32(((fp - 0xc)) as u32, (((rf(m, (iVar1 + 0x234)) * rf(m, (fp - 0x8))) + (rf(m, (iVar1 + 0x230)) * fVar6)) + (rf(m, (iVar1 + 0x22c)) * fVar5)).to_f32());
        m.set_f32(((fp - 0x8)) as u32, (((rf(m, (iVar1 + 0x238)) * fVar5) + (fVar6 * rf(m, (iVar1 + 0x23c)))) + (rf(m, (fp - 0x8)) * rf(m, (iVar1 + 0x240)))).to_f32());
        if mdl2 == 0 { break 'lab; }
        iVar1 = ri(m, (mdl3 + 0x2c4));
        fVar6 = (rf(m, (fp - 0x10)) - rf(m, (ri(m, (iVar1 + 0x2c0)) + 0x158)));
        fVar5 = (rf(m, (fp - 0xc)) - rf(m, (ri(m, (iVar1 + 0x2c0)) + 0x15c)));
        m.set_f32(((fp - 0x8)) as u32, (rf(m, (fp - 0x8)) - rf(m, (ri(m, (iVar1 + 0x2c0)) + 0x160))).to_f32());
        iVar2 = ri(m, (iVar1 + 0x2c0));
        m.set_f32(((fp - 0x10)) as u32, (((rf(m, (iVar2 + 0x120)) * rf(m, (fp - 0x8))) + (rf(m, (iVar2 + 0x11c)) * fVar5)) + (rf(m, (iVar2 + 0x118)) * fVar6)).to_f32());
        iVar2 = ri(m, (iVar1 + 0x2c0));
        m.set_f32(((fp - 0xc)) as u32, (((rf(m, (iVar2 + 0x130)) * rf(m, (fp - 0x8))) + (rf(m, (iVar2 + 0x12c)) * fVar5)) + (rf(m, (iVar2 + 0x128)) * fVar6)).to_f32());
        iVar2 = ri(m, (iVar1 + 0x2c0));
        m.set_f32(((fp - 0x8)) as u32, (((rf(m, (iVar2 + 0x138)) * fVar6) + (fVar5 * rf(m, (iVar2 + 0x13c)))) + (rf(m, (fp - 0x8)) * rf(m, (iVar2 + 0x140)))).to_f32());
        p_Var3 = ri(m, (iVar1 + 0x2c0));
        if mdl2 == p_Var3 { break 'lab; }
        iVar2 = ri(m, (iVar1 + 0x2c0));
        iVar4 = ri(m, (iVar1 + 0x2c0));
        fVar7 = (((((((rf(m, (fp - 0x8)) * rf(m, (p_Var3 + 0x198))) + (rf(m, (fp - 0xc)) * rf(m, (p_Var3 + 0x188)))) + (rf(m, (fp - 0x10)) * rf(m, (p_Var3 + 0x178)))) + rf(m, (p_Var3 + 0x1a8))) + rf(m, (ri(m, (iVar1 + 0x2c0)) + 0x1b8)))) - rf(m, (mdl2 + 0x1b8)));
        fVar6 = (((((((rf(m, (fp - 0x8)) * rf(m, (iVar2 + 0x19c))) + (rf(m, (fp - 0xc)) * rf(m, (iVar2 + 0x18c)))) + (rf(m, (fp - 0x10)) * rf(m, (iVar2 + 0x17c)))) + rf(m, (iVar2 + 0x1ac))) + rf(m, (ri(m, (iVar1 + 0x2c0)) + 0x1bc)))) - rf(m, (mdl2 + 0x1bc)));
        fVar5 = (((((((rf(m, (fp - 0x10)) * rf(m, (iVar4 + 0x180))) + (rf(m, (fp - 0xc)) * rf(m, (iVar4 + 0x190)))) + (rf(m, (fp - 0x8)) * rf(m, (iVar4 + 0x1a0)))) + rf(m, (iVar4 + 0x1b0))) + rf(m, (ri(m, (iVar1 + 0x2c0)) + 0x1c0)))) - rf(m, (mdl2 + 0x1c0)));
        m.set_f32(((fp - 0x10)) as u32, (((rf(m, (mdl2 + 0x180)) * fVar5) + (rf(m, (mdl2 + 0x178)) * fVar7)) + (rf(m, (mdl2 + 0x17c)) * fVar6)).to_f32());
        m.set_f32(((fp - 0xc)) as u32, (((rf(m, (mdl2 + 0x190)) * fVar5) + (rf(m, (mdl2 + 0x18c)) * fVar6)) + (rf(m, (mdl2 + 0x188)) * fVar7)).to_f32());
    } else {
        if mdl2 == mdl1 { break 'lab; }
        fVar7 = (((((((rf(m, (fp - 0xc)) * rf(m, (mdl1 + 0x188))) + (rf(m, (fp - 0x10)) * rf(m, (mdl1 + 0x178)))) + (rf(m, (fp - 0x8)) * rf(m, (mdl1 + 0x198)))) + rf(m, (mdl1 + 0x1a8))) + rf(m, (mdl1 + 0x1b8)))) - rf(m, (mdl2 + 0x1b8)));
        fVar6 = (((((((rf(m, (fp - 0xc)) * rf(m, (mdl1 + 0x18c))) + (rf(m, (fp - 0x10)) * rf(m, (mdl1 + 0x17c)))) + (rf(m, (fp - 0x8)) * rf(m, (mdl1 + 0x19c)))) + rf(m, (mdl1 + 0x1ac))) + rf(m, (mdl1 + 0x1bc)))) - rf(m, (mdl2 + 0x1bc)));
        fVar5 = (((((((rf(m, (fp - 0x10)) * rf(m, (mdl1 + 0x180))) + (rf(m, (fp - 0xc)) * rf(m, (mdl1 + 0x190)))) + (rf(m, (fp - 0x8)) * rf(m, (mdl1 + 0x1a0)))) + rf(m, (mdl1 + 0x1b0))) + rf(m, (mdl1 + 0x1c0)))) - rf(m, (mdl2 + 0x1c0)));
        m.set_f32(((fp - 0x10)) as u32, (((rf(m, (mdl2 + 0x180)) * fVar5) + (rf(m, (mdl2 + 0x178)) * fVar7)) + (rf(m, (mdl2 + 0x17c)) * fVar6)).to_f32());
        m.set_f32(((fp - 0xc)) as u32, (((rf(m, (mdl2 + 0x190)) * fVar5) + (rf(m, (mdl2 + 0x18c)) * fVar6)) + (rf(m, (mdl2 + 0x188)) * fVar7)).to_f32());
    }
    m.set_f32(((fp - 0x8)) as u32, (((fVar7 * rf(m, (mdl2 + 0x198))) + (fVar6 * rf(m, (mdl2 + 0x19c)))) + (fVar5 * rf(m, (mdl2 + 0x1a0)))).to_f32());
    }
    fVar5 = rf(m, (mdl4 + 0x16c)).abs();
    if fVar5.le(x(0.0f32)) {
        fVar5 = x(0.0f32);
    }
    if fVar5.lt(rf(m, (mdl4 + 0x170)).abs()) {
        fVar5 = rf(m, (mdl4 + 0x170)).abs();
    }
    if fVar5.lt(rf(m, (mdl4 + 0x174)).abs()) {
        fVar5 = rf(m, (mdl4 + 0x174)).abs();
    }
    m.set_f32(((fp - 0x34)) as u32, (rf(m, (ri(m, (fp - 0xbc)) + 0x16c)) * rf(m, (ri(m, (fp - 0xbc)) + 0x10))).to_f32());
    m.set_f32(((fp - 0x30)) as u32, (rf(m, (ri(m, (fp - 0xbc)) + 0x170)) * rf(m, (ri(m, (fp - 0xbc)) + 0x14))).to_f32());
    m.set_f32(((fp - 0x2c)) as u32, (rf(m, (ri(m, (fp - 0xbc)) + 0x174)) * rf(m, (ri(m, (fp - 0xbc)) + 0x18))).to_f32());
    m.set_f32(((fp - 0x40)) as u32, (rf(m, (ri(m, (fp - 0xbc)) + 0x16c)) * rf(m, (ri(m, (fp - 0xbc)) + 0x1c))).to_f32());
    m.set_f32(((fp - 0x3c)) as u32, (rf(m, (ri(m, (fp - 0xbc)) + 0x170)) * rf(m, (ri(m, (fp - 0xbc)) + 0x20))).to_f32());
    m.set_f32(((fp - 0x38)) as u32, (rf(m, (ri(m, (fp - 0xbc)) + 0x174)) * rf(m, (ri(m, (fp - 0xbc)) + 0x24))).to_f32());
    sVar10 = check_bsph_bbox_collision(m, [rf(m, (fp - 0x10)).to_f32(), rf(m, (fp - 0xc)).to_f32(), rf(m, (fp - 0x8)).to_f32()], (rf(m, (mdl4 + 0x2c)) * fVar5).to_f32(), [rf(m, (fp - 0x34)).to_f32(), rf(m, (fp - 0x30)).to_f32(), rf(m, (fp - 0x2c)).to_f32()], [rf(m, (fp - 0x40)).to_f32(), rf(m, (fp - 0x3c)).to_f32(), rf(m, (fp - 0x38)).to_f32()], (fp - 0x28) as u32, (fp - 0x64) as u32) as i16 as i32;
    if sVar10 != 0 {
        if mdl2 == 0 {
            fVar6 = ((((rf(m, (fp - 0x28)) * rf(m, (mdl3 + 0x178))) + (rf(m, (fp - 0x24)) * rf(m, (mdl3 + 0x188)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl3 + 0x198)))) + rf(m, (mdl3 + 0x1a8)));
            fVar9 = ((((rf(m, (fp - 0x24)) * rf(m, (mdl3 + 0x18c))) + (rf(m, (fp - 0x28)) * rf(m, (mdl3 + 0x17c)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl3 + 0x19c)))) + rf(m, (mdl3 + 0x1ac)));
            fVar7 = (rf(m, (mdl4 + 0x178)) * fVar6);
            fVar8 = (rf(m, (mdl4 + 0x17c)) * fVar9);
            fVar5 = ((((rf(m, (fp - 0x28)) * rf(m, (mdl3 + 0x180))) + (rf(m, (fp - 0x24)) * rf(m, (mdl3 + 0x190)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl3 + 0x1a0)))) + rf(m, (mdl3 + 0x1b0)));
        } else {
            fVar6 = ((((rf(m, (fp - 0x28)) * rf(m, (mdl2 + 0x178))) + (rf(m, (fp - 0x24)) * rf(m, (mdl2 + 0x188)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl2 + 0x198)))) + rf(m, (mdl2 + 0x1a8)));
            fVar9 = ((((rf(m, (fp - 0x24)) * rf(m, (mdl2 + 0x18c))) + (rf(m, (fp - 0x28)) * rf(m, (mdl2 + 0x17c)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl2 + 0x19c)))) + rf(m, (mdl2 + 0x1ac)));
            fVar7 = (rf(m, (mdl4 + 0x178)) * fVar6);
            fVar8 = (rf(m, (mdl4 + 0x17c)) * fVar9);
            fVar5 = ((((rf(m, (fp - 0x28)) * rf(m, (mdl2 + 0x180))) + (rf(m, (fp - 0x24)) * rf(m, (mdl2 + 0x190)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl2 + 0x1a0)))) + rf(m, (mdl2 + 0x1b0)));
        }
        m.set_f32(((fp - 0x28)) as u32, (((rf(m, (mdl4 + 0x180)) * fVar5) + fVar7) + fVar8).to_f32());
        m.set_f32(((fp - 0x24)) as u32, (((rf(m, (mdl4 + 0x190)) * fVar5) + (rf(m, (mdl4 + 0x18c)) * fVar9)) + (rf(m, (mdl4 + 0x188)) * fVar6)).to_f32());
        m.set_f32(((fp - 0x20)) as u32, (((fVar5 * rf(m, (mdl4 + 0x1a0))) + (fVar6 * rf(m, (mdl4 + 0x198)))) + (fVar9 * rf(m, (mdl4 + 0x19c)))).to_f32());
        fVar5 = (((rf(m, (fp - 0x20)) * rf(m, (fp - 0x20))) + (rf(m, (fp - 0x28)) * rf(m, (fp - 0x28)))) + (rf(m, (fp - 0x24)) * rf(m, (fp - 0x24)))).sqrt();
        m.set_f32(((fp - 0x58)) as u32, rf(m, (fp - 0x28)).to_f32());
        m.set_f32(((fp - 0x54)) as u32, rf(m, (fp - 0x24)).to_f32());
        m.set_f32(((fp - 0x50)) as u32, rf(m, (fp - 0x20)).to_f32());
        if x(0.0001f64).lt(fVar5.abs()) {
            m.set_f32(((fp - 0x58)) as u32, (rf(m, (fp - 0x28)) / fVar5).to_f32());
            m.set_f32(((fp - 0x54)) as u32, (rf(m, (fp - 0x24)) / fVar5).to_f32());
            m.set_f32(((fp - 0x50)) as u32, (rf(m, (fp - 0x20)) / fVar5).to_f32());
        }
        m.set_i32(((fp - 0x4c)) as u32, 0);
        lVar11 = fpatan(rf(m, (fp - 0x50)), rf(m, (fp - 0x58)));
        m.set_f32(((fp - 0x48)) as u32, lVar11.to_f32());
        lVar11 = fpatan(rf(m, (fp - 0x58)), rf(m, (fp - 0x54)));
        m.set_f32(((fp - 0x44)) as u32, (-lVar11).to_f32());
        fVar7 = (((((((rf(m, (fp - 0x28)) * rf(m, (mdl4 + 0x178))) + (rf(m, (fp - 0x24)) * rf(m, (mdl4 + 0x188)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl4 + 0x198)))) + rf(m, (mdl4 + 0x1a8))) + rf(m, (mdl4 + 0x1b8)))) - rf(m, (mdl3 + 0x1b8)));
        fVar6 = (((((((rf(m, (fp - 0x24)) * rf(m, (mdl4 + 0x18c))) + (rf(m, (fp - 0x28)) * rf(m, (mdl4 + 0x17c)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl4 + 0x19c)))) + rf(m, (mdl4 + 0x1ac))) + rf(m, (mdl4 + 0x1bc)))) - rf(m, (mdl3 + 0x1bc)));
        fVar5 = (((((((rf(m, (fp - 0x28)) * rf(m, (mdl4 + 0x180))) + (rf(m, (fp - 0x24)) * rf(m, (mdl4 + 0x190)))) + (rf(m, (fp - 0x20)) * rf(m, (mdl4 + 0x1a0)))) + rf(m, (mdl4 + 0x1b0))) + rf(m, (mdl4 + 0x1c0)))) - rf(m, (mdl3 + 0x1c0)));
        m.set_f32(((fp - 0x1c)) as u32, (((rf(m, (mdl3 + 0x180)) * fVar5) + (rf(m, (mdl3 + 0x178)) * fVar7)) + (rf(m, (mdl3 + 0x17c)) * fVar6)).to_f32());
        m.set_f32(((fp - 0x18)) as u32, (((rf(m, (mdl3 + 0x190)) * fVar5) + (rf(m, (mdl3 + 0x18c)) * fVar6)) + (rf(m, (mdl3 + 0x188)) * fVar7)).to_f32());
        m.set_f32(((fp - 0x14)) as u32, (((fVar7 * rf(m, (mdl3 + 0x198))) + (fVar6 * rf(m, (mdl3 + 0x19c)))) + (fVar5 * rf(m, (mdl3 + 0x1a0)))).to_f32());
        if add_vertex != 0 {
            surface_of_contact_add_vertex(m, rf(m, (fp - 0x1c)).to_f32().to_bits(), rf(m, (fp - 0x18)).to_f32().to_bits(), rf(m, (fp - 0x14)).to_f32().to_bits(), ri(m, fp - 0x8c) as u32, soc as u32);
        }
        m.set_f32(((soc + 0x10)) as u32, rf(m, (fp - 0x28)).to_f32());
        m.set_f32(((soc + 0x14)) as u32, rf(m, (fp - 0x24)).to_f32());
        m.set_i32(((soc + 0x74)) as u32, 0);
        m.set_f32(((soc + 0x18)) as u32, rf(m, (fp - 0x20)).to_f32());
        m.set_f32(((soc + 0x1c)) as u32, rf(m, (fp - 0x28)).to_f32());
        m.set_f32(((soc + 0x20)) as u32, rf(m, (fp - 0x24)).to_f32());
        m.set_f32(((soc + 0x24)) as u32, rf(m, (fp - 0x20)).to_f32());
        m.set_f32(((soc + 0x28)) as u32, rf(m, (fp - 0x28)).to_f32());
        m.set_f32(((soc + 0x2c)) as u32, rf(m, (fp - 0x24)).to_f32());
        m.set_f32(((soc + 0x30)) as u32, rf(m, (fp - 0x20)).to_f32());
        o.si_matrix_xyz_rotation(m, ((fp - 0x4c)) as u32, ((soc + 0x34)) as u32);
    }
    return sVar10;

}

/// [soft 0x084a42c0 detectDynaBboxcollision]: the box-against-box test of two bodies of the
/// simulation (eight corners against the other body's box); see the finding for what is read from the code and what is a guess.

#[allow(clippy::too_many_arguments)]
pub fn detect_dyna_bbox_collision(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, mdl3: u32, mdl4: u32, add_vertex: i16, soc: u32) -> i32 {
    let fr = m.local(0x224);
    let r = detect_dyna_bbox_collision_body(m, o, fr + 0x224, mdl1 as i32, mdl2 as i32, mdl3 as i32, mdl4 as i32, add_vertex as i32, soc as i32);
    m.release(0x224);
    r as i32
}

#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::too_many_arguments)]
fn detect_dyna_bbox_collision_body(m: &mut Mem, o: &mut dyn Outside, fp: u32, mdl1: i32, mdl2: i32, mdl3: i32, mdl4: i32, add_vertex: i32, soc: i32) -> i32 {
    let fp = fp as i32;
    let mut fVar1 = x(0.0f32);
    let mut fVar2 = x(0.0f32);
    let mut fVar3 = x(0.0f32);
    let mut iVar4: i32 = 0;
    let mut iVar5: i32 = 0;
    let mut iVar6: i32 = 0;
    let mut iVar7: i32 = 0;
    let mut iVar8: i32 = 0;
    let mut fVar9 = x(0.0f32);
    let mut fVar10 = x(0.0f32);
    let mut fVar11 = x(0.0f32);
    let mut p_Var12: i32 = 0;
    let mut pfVar13: i32 = 0;
    let mut iVar14: i32 = 0;
    let mut lVar15 = x(0.0f32);
    p_Var12 = mdl2;
    if mdl2 == 0 {
        p_Var12 = mdl3;
    }
    m.set_i32(((fp - 0x214)) as u32, mdl2);
    if (mdl2 == 0) || (ri(m, (mdl2 + 0x2f4)) == 0) {
        m.set_i32(((fp - 0x214)) as u32, mdl3);
    }
    m.set_i32(((fp - 0x1f4)) as u32, ri(m, (p_Var12 + 0x10)));
    m.set_i32(((fp - 0x1f0)) as u32, ri(m, (p_Var12 + 0x1c)));
    m.set_f32(((fp - 0x204) + 4 * (2)) as u32, rf(m, (p_Var12 + 0x14)).to_f32());
    m.set_f32(((fp - 0x204) + 4 * (3)) as u32, rf(m, (p_Var12 + 0x20)).to_f32());
    m.set_f32(((fp - 0x204) + 4 * (0)) as u32, rf(m, (p_Var12 + 0x18)).to_f32());
    m.set_f32(((fp - 0x204) + 4 * (1)) as u32, rf(m, (p_Var12 + 0x24)).to_f32());
    iVar14 = 0;
    iVar4 = ri(m, (mdl3 + 0x300));
    m.set_i32(((fp - 0x20c)) as u32, 7);
    pfVar13 = (fp - 0x1cc) + 4 * 2;
    loop {
        m.set_f32(((fp - 0x28)) as u32, x(-1.0f32).to_f32());
        m.set_f32(((fp - 0x24)) as u32, x(-1.0f32).to_f32());
        m.set_f32(((fp - 0x20)) as u32, x(-1.0f32).to_f32());
        if 0 < ri(m, (0x086d7220 + iVar14)) {
            m.set_f32(((fp - 0x28)) as u32, x(1.0f32).to_f32());
        }
        if 0 < ri(m, (0x086d7224 + iVar14)) {
            m.set_f32(((fp - 0x24)) as u32, x(1.0f32).to_f32());
        }
        if 0 < ri(m, (0x086d7228 + iVar14)) {
            m.set_f32(((fp - 0x20)) as u32, x(1.0f32).to_f32());
        }
        m.set_f32(((fp - 0x10)) as u32, rf(m, (fp - 0x204) + 4 * (ri(m, (0x086d7220 + iVar14)) + 4)).to_f32());
        m.set_f32(((fp - 0xc)) as u32, rf(m, (fp - 0x204) + 4 * (ri(m, (0x086d7224 + iVar14)) + 2)).to_f32());
        m.set_f32(((fp - 0x1c)) as u32, (rf(m, (fp - 0x204) + 4 * (ri(m, (0x086d7220 + iVar14)) + 4)) * rf(m, (p_Var12 + 0x16c))).to_f32());
        m.set_f32(((fp - 0x10)) as u32, rf(m, (fp - 0x1c)).to_f32());
        m.set_f32(((fp - 0x18)) as u32, (rf(m, (fp - 0x204) + 4 * (ri(m, (0x086d7224 + iVar14)) + 2)) * rf(m, (p_Var12 + 0x170))).to_f32());
        m.set_f32(((fp - 0xc)) as u32, rf(m, (fp - 0x18)).to_f32());
        m.set_f32(((fp - 0x14)) as u32, (rf(m, (fp - 0x204) + 4 * (ri(m, (0x086d7228 + iVar14)))) * rf(m, (p_Var12 + 0x174))).to_f32());
        m.set_f32(((fp - 0x34)) as u32, rf(m, (fp - 0x28)).to_f32());
        m.set_f32(((fp - 0x30)) as u32, rf(m, (fp - 0x24)).to_f32());
        m.set_f32(((fp - 0x2c)) as u32, rf(m, (fp - 0x20)).to_f32());
        m.set_f32(((fp - 0x8)) as u32, rf(m, (fp - 0x14)).to_f32());
        if ri(m, (mdl1 + 0x2ec)) == 2 {
            if mdl2 != 0 {
                iVar5 = ri(m, (mdl3 + 0x2c4));
                if mdl2 != ri(m, (iVar5 + 0x2c0)) {
                    fVar3 = (((((((rf(m, (fp - 0x18)) * rf(m, (mdl2 + 0x188))) + (rf(m, (fp - 0x1c)) * rf(m, (mdl2 + 0x178)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl2 + 0x198)))) + rf(m, (mdl2 + 0x1a8))) + rf(m, (mdl2 + 0x1b8)))) - rf(m, (ri(m, (iVar5 + 0x2c0)) + 0x1b8)));
                    fVar2 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl2 + 0x17c))) + (rf(m, (fp - 0x18)) * rf(m, (mdl2 + 0x18c)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl2 + 0x19c)))) + rf(m, (mdl2 + 0x1ac))) + rf(m, (mdl2 + 0x1bc)))) - rf(m, (ri(m, (iVar5 + 0x2c0)) + 0x1bc)));
                    fVar1 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl2 + 0x180))) + (rf(m, (fp - 0x18)) * rf(m, (mdl2 + 0x190)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl2 + 0x1a0)))) + rf(m, (mdl2 + 0x1b0))) + rf(m, (mdl2 + 0x1c0)))) - rf(m, (ri(m, (iVar5 + 0x2c0)) + 0x1c0)));
                    iVar6 = ri(m, (iVar5 + 0x2c0));
                    m.set_f32(((fp - 0x10)) as u32, (((rf(m, (iVar6 + 0x180)) * fVar1) + (rf(m, (iVar6 + 0x17c)) * fVar2)) + (rf(m, (iVar6 + 0x178)) * fVar3)).to_f32());
                    iVar6 = ri(m, (iVar5 + 0x2c0));
                    m.set_f32(((fp - 0xc)) as u32, (((rf(m, (iVar6 + 0x190)) * fVar1) + (rf(m, (iVar6 + 0x18c)) * fVar2)) + (rf(m, (iVar6 + 0x188)) * fVar3)).to_f32());
                    iVar5 = ri(m, (iVar5 + 0x2c0));
                    m.set_f32(((fp - 0x8)) as u32, (((fVar3 * rf(m, (iVar5 + 0x198))) + (fVar2 * rf(m, (iVar5 + 0x19c)))) + (fVar1 * rf(m, (iVar5 + 0x1a0)))).to_f32());
                }
                iVar5 = ri(m, (mdl3 + 0x2c4));
                iVar6 = ri(m, (iVar5 + 0x2c0));
                iVar7 = ri(m, (iVar5 + 0x2c0));
                fVar2 = (rf(m, (fp - 0x10)) * rf(m, (iVar7 + 0x11c)));
                iVar8 = ri(m, (iVar5 + 0x2c0));
                fVar1 = (rf(m, (fp - 0x10)) * rf(m, (iVar8 + 0x120)));
                fVar3 = (rf(m, (fp - 0xc)) * rf(m, (iVar8 + 0x130)));
                m.set_f32(((fp - 0x10)) as u32, (((((rf(m, (fp - 0x8)) * rf(m, (iVar6 + 0x138))) + (rf(m, (fp - 0x10)) * rf(m, (iVar6 + 0x118)))) + (rf(m, (fp - 0xc)) * rf(m, (iVar6 + 0x128)))) + rf(m, (iVar6 + 0x148))) + rf(m, (ri(m, (iVar5 + 0x2c0)) + 0x158))).to_f32());
                m.set_f32(((fp - 0xc)) as u32, (((((rf(m, (fp - 0x8)) * rf(m, (iVar7 + 0x13c))) + (rf(m, (fp - 0xc)) * rf(m, (iVar7 + 0x12c)))) + fVar2) + rf(m, (iVar7 + 0x14c))) + rf(m, (ri(m, (iVar5 + 0x2c0)) + 0x15c))).to_f32());
                m.set_f32(((fp - 0x8)) as u32, ((((fVar1 + fVar3) + (rf(m, (fp - 0x8)) * rf(m, (iVar8 + 0x140)))) + rf(m, (iVar8 + 0x150))) + rf(m, (ri(m, (iVar5 + 0x2c0)) + 0x160))).to_f32());
            }
            m.set_f32(((fp - 0x1c)) as u32, ((((rf(m, (iVar4 + 0x1e0)) * rf(m, (fp - 0x8))) + (rf(m, (iVar4 + 0x1d8)) * rf(m, (fp - 0x10)))) + (rf(m, (iVar4 + 0x1dc)) * rf(m, (fp - 0xc)))) + rf(m, (iVar4 + 0x244))).to_f32());
            m.set_f32(((fp - 0x18)) as u32, ((((rf(m, (iVar4 + 0x1ec)) * rf(m, (fp - 0x8))) + (rf(m, (iVar4 + 0x1e8)) * rf(m, (fp - 0xc)))) + (rf(m, (iVar4 + 0x1e4)) * rf(m, (fp - 0x10)))) + rf(m, (iVar4 + 0x248))).to_f32());
            m.set_f32(((fp - 0x14)) as u32, ((((rf(m, (fp - 0x10)) * rf(m, (iVar4 + 0x1f0))) + (rf(m, (fp - 0xc)) * rf(m, (iVar4 + 0x1f4)))) + (rf(m, (fp - 0x8)) * rf(m, (iVar4 + 0x1f8)))) + rf(m, (iVar4 + 0x24c))).to_f32());
        } else if mdl2 != mdl1 {
            fVar3 = (((((((rf(m, (fp - 0x18)) * rf(m, (mdl2 + 0x188))) + (rf(m, (fp - 0x1c)) * rf(m, (mdl2 + 0x178)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl2 + 0x198)))) + rf(m, (mdl2 + 0x1a8))) + rf(m, (mdl2 + 0x1b8)))) - rf(m, (mdl1 + 0x1b8)));
            fVar2 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl2 + 0x17c))) + (rf(m, (fp - 0x18)) * rf(m, (mdl2 + 0x18c)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl2 + 0x19c)))) + rf(m, (mdl2 + 0x1ac))) + rf(m, (mdl2 + 0x1bc)))) - rf(m, (mdl1 + 0x1bc)));
            fVar1 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl2 + 0x180))) + (rf(m, (fp - 0x18)) * rf(m, (mdl2 + 0x190)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl2 + 0x1a0)))) + rf(m, (mdl2 + 0x1b0))) + rf(m, (mdl2 + 0x1c0)))) - rf(m, (mdl1 + 0x1c0)));
            m.set_f32(((fp - 0x1c)) as u32, (((rf(m, (mdl1 + 0x180)) * fVar1) + (rf(m, (mdl1 + 0x178)) * fVar3)) + (rf(m, (mdl1 + 0x17c)) * fVar2)).to_f32());
            m.set_f32(((fp - 0x18)) as u32, (((rf(m, (mdl1 + 0x190)) * fVar1) + (rf(m, (mdl1 + 0x18c)) * fVar2)) + (rf(m, (mdl1 + 0x188)) * fVar3)).to_f32());
            m.set_f32(((fp - 0x14)) as u32, (((fVar1 * rf(m, (mdl1 + 0x1a0))) + (fVar3 * rf(m, (mdl1 + 0x198)))) + (fVar2 * rf(m, (mdl1 + 0x19c)))).to_f32());
        }
        fVar3 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl1 + 0x178))) + (rf(m, (fp - 0x18)) * rf(m, (mdl1 + 0x188)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl1 + 0x198)))) + rf(m, (mdl1 + 0x1a8))) + rf(m, (mdl1 + 0x1b8)))) - rf(m, (mdl4 + 0x1b8)));
        fVar2 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl1 + 0x17c))) + (rf(m, (fp - 0x18)) * rf(m, (mdl1 + 0x18c)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl1 + 0x19c)))) + rf(m, (mdl1 + 0x1ac))) + rf(m, (mdl1 + 0x1bc)))) - rf(m, (mdl4 + 0x1bc)));
        fVar1 = (((((((rf(m, (fp - 0x1c)) * rf(m, (mdl1 + 0x180))) + (rf(m, (fp - 0x18)) * rf(m, (mdl1 + 0x190)))) + (rf(m, (fp - 0x14)) * rf(m, (mdl1 + 0x1a0)))) + rf(m, (mdl1 + 0x1b0))) + rf(m, (mdl1 + 0x1c0)))) - rf(m, (mdl4 + 0x1c0)));
        m.set_f32(((fp - 0x1c)) as u32, (((rf(m, (mdl4 + 0x180)) * fVar1) + (rf(m, (mdl4 + 0x178)) * fVar3)) + (rf(m, (mdl4 + 0x17c)) * fVar2)).to_f32());
        m.set_f32(((fp - 0x18)) as u32, (((rf(m, (mdl4 + 0x190)) * fVar1) + (rf(m, (mdl4 + 0x18c)) * fVar2)) + (rf(m, (mdl4 + 0x188)) * fVar3)).to_f32());
        iVar14 = iVar14 + 12;
        m.set_f32(((fp - 0x14)) as u32, (((fVar3 * rf(m, (mdl4 + 0x198))) + (fVar2 * rf(m, (mdl4 + 0x19c)))) + (fVar1 * rf(m, (mdl4 + 0x1a0)))).to_f32());
        m.set_f32((pfVar13 + 4 * (0x2e)) as u32, rf(m, (fp - 0x1c)).to_f32());
        m.set_f32((pfVar13 + 4 * (0x2f)) as u32, rf(m, (fp - 0x18)).to_f32());
        m.set_f32((pfVar13 + 4 * (0x30)) as u32, rf(m, (fp - 0x14)).to_f32());
        m.set_f32((pfVar13 + 4 * (0x16)) as u32, rf(m, (fp - 0x10)).to_f32());
        m.set_f32((pfVar13 + 4 * (0x17)) as u32, rf(m, (fp - 0xc)).to_f32());
        m.set_f32((pfVar13 + 4 * (0x18)) as u32, rf(m, (fp - 0x8)).to_f32());
        m.set_f32((pfVar13 + 4 * ((-2))) as u32, rf(m, (fp - 0x34)).to_f32());
        m.set_f32((pfVar13 + 4 * ((-1))) as u32, rf(m, (fp - 0x30)).to_f32());
        m.set_f32((pfVar13) as u32, rf(m, (fp - 0x2c)).to_f32());
        pfVar13 = pfVar13 + 4 * 3;
        m.set_i32(((fp - 0x20c)) as u32, ri(m, (fp - 0x20c)) + (-1));
        if !((-1) < ri(m, (fp - 0x20c))) { break; }
    }
    m.set_f32(((fp - 0x38)) as u32, x(0.0f32).to_f32());
    m.set_f32(((fp - 0x3c)) as u32, x(0.0f32).to_f32());
    m.set_f32(((fp - 0x40)) as u32, x(0.0f32).to_f32());
    m.set_f32(((fp - 0x4c)) as u32, x(0.0f32).to_f32());
    m.set_f32(((fp - 0x48)) as u32, x(1.0f32).to_f32());
    m.set_f32(((fp - 0x44)) as u32, x(0.0f32).to_f32());
    m.set_i32(((fp - 0x210)) as u32, 0);
    if rs(m, (soc + 0x78)) != (-1) { return (0 < ri(m, (fp - 0x210))) as i32; }
    m.set_i32(((fp - 0x20c)) as u32, 0);
    m.set_i32(((fp - 0x224)) as u32, 0);
    m.set_f32(((fp - 0x78)) as u32, (rf(m, (mdl4 + 0x20)) * rf(m, (mdl4 + 0x170))).to_f32());
    loop {
        fVar2 = (rf(m, (fp - 0x78)) + x(0.05f64));
        fVar1 = rf(m, ((fp - 0x108) + ri(m, (fp - 0x224))));
        if (((((fVar1.lt(fVar2)) as i32 | ((((((((((fVar1.is_nan() || fVar2.is_nan())) as i32) & 0xffff) << 10)) & 0xffff) >> 8)) & 0xff) | ((((((((((fVar1.eq(fVar2))) as i32) & 0xffff) << 14)) & 0xffff) >> 8)) & 0xff)) - 1)) & 0xff) < 0x40 {
            fVar1 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224))));
            fVar2 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 4));
            fVar3 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 8));
            m.set_f32(((fp - 0x1e8)) as u32, (((rf(m, (iVar4 + 0x1e0)) * fVar3) + (rf(m, (iVar4 + 0x1d8)) * fVar1)) + (rf(m, (iVar4 + 0x1dc)) * fVar2)).to_f32());
            m.set_f32(((fp - 0x1e4)) as u32, (((rf(m, (iVar4 + 0x1ec)) * fVar3) + (rf(m, (iVar4 + 0x1e8)) * fVar2)) + (rf(m, (iVar4 + 0x1e4)) * fVar1)).to_f32());
            m.set_f32(((fp - 0x1e0)) as u32, (((fVar3 * rf(m, (iVar4 + 0x1f8))) + (fVar1 * rf(m, (iVar4 + 0x1f0)))) + (fVar2 * rf(m, (iVar4 + 0x1f4)))).to_f32());
            m.set_f32(((fp - 0x58)) as u32, rf(m, (fp - 0x1e8)).to_f32());
            m.set_f32(((fp - 0x54)) as u32, rf(m, (fp - 0x1e4)).to_f32());
            m.set_f32(((fp - 0x50)) as u32, rf(m, (fp - 0x1e0)).to_f32());
            crate::settings::si_dyna_linear_speed_compute(m, (mdl3) as u32, rf(m, (fp - 0x1e8)).to_f32(), rf(m, (fp - 0x1e4)).to_f32(), rf(m, (fp - 0x1e0)).to_f32(), ((fp - 0x64)) as u32);
            fVar2 = rf(m, (fp - 0x60));
            fVar1 = rf(m, (fp - 0x64));
            let mut stage = 0;
            if ri(m, (mdl1 + 0x2ec)) == 2 {
                fVar1 = ((((rf(m, (fp - 0x64)) * rf(m, (mdl1 + 0x178))) + (rf(m, (fp - 0x60)) * rf(m, (mdl1 + 0x188)))) + (rf(m, (fp - 0x5c)) * rf(m, (mdl1 + 0x198)))) + rf(m, (mdl1 + 0x1a8)));
                fVar11 = ((rf(m, (fp - 0x64)) * rf(m, (mdl1 + 0x180))) + (rf(m, (fp - 0x60)) * rf(m, (mdl1 + 0x190))));
                fVar9 = (rf(m, (mdl4 + 0x178)) * fVar1);
                fVar2 = ((((rf(m, (fp - 0x64)) * rf(m, (mdl1 + 0x17c))) + (rf(m, (fp - 0x60)) * rf(m, (mdl1 + 0x18c)))) + (rf(m, (fp - 0x5c)) * rf(m, (mdl1 + 0x19c)))) + rf(m, (mdl1 + 0x1ac)));
                fVar3 = rf(m, (mdl1 + 0x1a0));
                fVar10 = (rf(m, (mdl4 + 0x17c)) * fVar2);
                p_Var12 = mdl1;
                stage = 2;
            } else if ri(m, (mdl1 + 0x2ec)) == 12 {
                p_Var12 = ri(m, (mdl1 + 0x2bc));
                if p_Var12 != 0 {
                    fVar1 = ((((rf(m, (fp - 0x5c)) * rf(m, (p_Var12 + 0x198))) + (rf(m, (fp - 0x64)) * rf(m, (p_Var12 + 0x178)))) + (rf(m, (fp - 0x60)) * rf(m, (p_Var12 + 0x188)))) + rf(m, (p_Var12 + 0x1a8)));
                    fVar2 = ((((rf(m, (fp - 0x5c)) * rf(m, (p_Var12 + 0x19c))) + (rf(m, (fp - 0x60)) * rf(m, (p_Var12 + 0x18c)))) + (rf(m, (fp - 0x64)) * rf(m, (p_Var12 + 0x17c)))) + rf(m, (p_Var12 + 0x1ac)));
                    fVar3 = rf(m, (p_Var12 + 0x1a0));
                    fVar10 = (rf(m, (mdl4 + 0x17c)) * fVar2);
                    fVar11 = ((rf(m, (fp - 0x64)) * rf(m, (p_Var12 + 0x180))) + (rf(m, (fp - 0x60)) * rf(m, (p_Var12 + 0x190))));
                    fVar9 = (rf(m, (mdl4 + 0x178)) * fVar1);
                    stage = 2;
                } else {
                m.set_f32(((fp - 0x64)) as u32, (((rf(m, (mdl4 + 0x180)) * rf(m, (fp - 0x5c))) + (rf(m, (mdl4 + 0x178)) * rf(m, (fp - 0x64)))) + (rf(m, (mdl4 + 0x17c)) * rf(m, (fp - 0x60)))).to_f32());
                m.set_f32(((fp - 0x60)) as u32, (((rf(m, (mdl4 + 0x190)) * rf(m, (fp - 0x5c))) + (rf(m, (mdl4 + 0x18c)) * rf(m, (fp - 0x60)))) + (rf(m, (mdl4 + 0x188)) * fVar1)).to_f32());
                fVar1 = (fVar1 * rf(m, (mdl4 + 0x198)));
                fVar2 = (fVar2 * rf(m, (mdl4 + 0x19c)));
                m.set_f32(((fp - 0x5c)) as u32, (rf(m, (fp - 0x5c)) * rf(m, (mdl4 + 0x1a0))).to_f32());
                stage = 1;
                }
            }
            if stage == 2 {
                m.set_f32(((fp - 0x5c)) as u32, ((fVar11 + (rf(m, (fp - 0x5c)) * fVar3)) + rf(m, (p_Var12 + 0x1b0))).to_f32());
                m.set_f32(((fp - 0x64)) as u32, (((rf(m, (mdl4 + 0x180)) * rf(m, (fp - 0x5c))) + fVar9) + fVar10).to_f32());
                m.set_f32(((fp - 0x60)) as u32, (((rf(m, (mdl4 + 0x190)) * rf(m, (fp - 0x5c))) + (rf(m, (mdl4 + 0x18c)) * fVar2)) + (rf(m, (mdl4 + 0x188)) * fVar1)).to_f32());
                fVar1 = (fVar1 * rf(m, (mdl4 + 0x198)));
                fVar2 = (fVar2 * rf(m, (mdl4 + 0x19c)));
                m.set_f32(((fp - 0x5c)) as u32, (rf(m, (fp - 0x5c)) * rf(m, (mdl4 + 0x1a0))).to_f32());
            }
            if stage >= 1 {
                m.set_f32(((fp - 0x5c)) as u32, ((rf(m, (fp - 0x5c)) + fVar1) + fVar2).to_f32());
            }
            if ((((((rf(m, (fp - 0x60)).lt(x(0.0f32))) as i32 | (((((((((rf(m, (fp - 0x60)).is_nan()) as i32) & 0xffff) << 10)) & 0xffff) >> 8)) & 0xff) | ((((((((((rf(m, (fp - 0x60)).eq(x(0.0f32)))) as i32) & 0xffff) << 14)) & 0xffff) >> 8)) & 0xff)) - 1)) & 0xff) < 0x40) && ({ m.set_i32(((fp - 0x210)) as u32, ri(m, (fp - 0x210)) + 1); add_vertex != 0 }) {
                m.set_f32(((fp - 0x40)) as u32, (rf(m, (fp - 0x40)) + rf(m, ((fp - 0x1cc) + ri(m, (fp - 0x224))))).to_f32());
                m.set_f32(((fp - 0x3c)) as u32, (rf(m, (fp - 0x3c)) + rf(m, ((fp - 0x1cc) + ri(m, (fp - 0x224)) + 4))).to_f32());
                m.set_f32(((fp - 0x38)) as u32, (rf(m, (fp - 0x38)) + rf(m, ((fp - 0x1cc) + ri(m, (fp - 0x224)) + 8))).to_f32());
                if (ri(m, (mdl1 + 0x2ec)) == 12) && (mdl2 != mdl1) {
                    fVar1 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224))));
                    fVar2 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 4));
                    fVar3 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 8));
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)))) as u32, ((((fVar3 * rf(m, (mdl2 + 0x198))) + (fVar2 * rf(m, (mdl2 + 0x188)))) + (fVar1 * rf(m, (mdl2 + 0x178)))) + rf(m, (mdl2 + 0x1a8))).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)) as u32, ((((fVar3 * rf(m, (mdl2 + 0x19c))) + (fVar1 * rf(m, (mdl2 + 0x17c)))) + (fVar2 * rf(m, (mdl2 + 0x18c)))) + rf(m, (mdl2 + 0x1ac))).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 8)) as u32, ((((fVar3 * rf(m, (mdl2 + 0x1a0))) + (fVar1 * rf(m, (mdl2 + 0x180)))) + (fVar2 * rf(m, (mdl2 + 0x190)))) + rf(m, (mdl2 + 0x1b0))).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)))) as u32, (rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)))) + rf(m, (mdl2 + 0x1b8))).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)) as u32, (rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)) + rf(m, (mdl2 + 0x1bc))).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 8)) as u32, (rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 8)) + rf(m, (mdl2 + 0x1c0))).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)))) as u32, (rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)))) - rf(m, (mdl1 + 0x1b8))).to_f32());
                    fVar2 = (rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)) - rf(m, (mdl1 + 0x1bc)));
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)) as u32, fVar2.to_f32());
                    fVar1 = rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224))));
                    fVar3 = (rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 8)) - rf(m, (mdl1 + 0x1c0)));
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)))) as u32, (((rf(m, (mdl1 + 0x180)) * fVar3) + (rf(m, (mdl1 + 0x178)) * fVar1)) + (rf(m, (mdl1 + 0x17c)) * fVar2)).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)) as u32, (((rf(m, (mdl1 + 0x190)) * fVar3) + (rf(m, (mdl1 + 0x18c)) * fVar2)) + (rf(m, (mdl1 + 0x188)) * fVar1)).to_f32());
                    m.set_f32((((fp - 0x16c) + ri(m, (fp - 0x224)) + 8)) as u32, (((fVar1 * rf(m, (mdl1 + 0x198))) + (fVar2 * rf(m, (mdl1 + 0x19c)))) + (fVar3 * rf(m, (mdl1 + 0x1a0)))).to_f32());
                }
                m.set_f32(((fp - 0x1e8)) as u32, rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)))).to_f32());
                m.set_f32(((fp - 0x1e4)) as u32, rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 4)).to_f32());
                m.set_f32(((fp - 0x1e0)) as u32, rf(m, ((fp - 0x16c) + ri(m, (fp - 0x224)) + 8)).to_f32());
                surface_of_contact_add_vertex(m, rf(m, (fp - 0x1e8)).to_f32().to_bits(), rf(m, (fp - 0x1e4)).to_f32().to_bits(), rf(m, (fp - 0x1e0)).to_f32().to_bits(), ri(m, fp - 0x214) as u32, soc as u32);
            }
        }
        m.set_i32(((fp - 0x224)) as u32, ri(m, (fp - 0x224)) + 12);
        m.set_i32(((fp - 0x20c)) as u32, ri(m, (fp - 0x20c)) + 1);
        if !(ri(m, (fp - 0x20c)) < 8) { break; }
    }
    o.si_matrix_copy(m, (0x086dc160_u32) as u32, (soc + 0x34) as u32);
    m.set_i32(((soc + 0x30)) as u32, 0);
    m.set_i32(((soc + 0x2c)) as u32, 0);
    m.set_i32(((soc + 0x28)) as u32, 0);
    if (ri(m, (fp - 0x210)) < 1) || (add_vertex == 0) { return (0 < ri(m, (fp - 0x210))) as i32; }
    fVar1 = (x(1.0f32) / F80::from_i32(ri(m, (fp - 0x210))));
    m.set_f32(((fp - 0x40)) as u32, (rf(m, (fp - 0x40)) * fVar1).to_f32());
    m.set_f32(((fp - 0x3c)) as u32, (fVar1 * rf(m, (fp - 0x3c))).to_f32());
    m.set_f32(((fp - 0x38)) as u32, (fVar1 * rf(m, (fp - 0x38))).to_f32());
    m.set_f32(((fp - 0x1d0)) as u32, (((rf(m, (fp - 0x3c)) * rf(m, (fp - 0x3c))) + (rf(m, (fp - 0x40)) * rf(m, (fp - 0x40)))) + (rf(m, (fp - 0x38)) * rf(m, (fp - 0x38)))).sqrt().to_f32());
    if x(0.0001f64).lt(rf(m, (fp - 0x1d0)).abs()) {
        m.set_f32(((fp - 0x40)) as u32, (rf(m, (fp - 0x40)) / rf(m, (fp - 0x1d0))).to_f32());
        m.set_f32(((fp - 0x3c)) as u32, (rf(m, (fp - 0x3c)) / rf(m, (fp - 0x1d0))).to_f32());
        m.set_f32(((fp - 0x38)) as u32, (rf(m, (fp - 0x38)) / rf(m, (fp - 0x1d0))).to_f32());
    }
    m.set_f32(((soc + 0x10)) as u32, (-rf(m, (fp - 0x40))).to_f32());
    m.set_f32(((soc + 0x14)) as u32, (-rf(m, (fp - 0x3c))).to_f32());
    m.set_f32(((soc + 0x18)) as u32, (-rf(m, (fp - 0x38))).to_f32());
    if ri(m, (mdl1 + 0x2ec)) == 2 {
        fVar1 = ((((rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x178))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x188)))) + (rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x198)))) + rf(m, (mdl4 + 0x1a8)));
        fVar2 = ((((rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x18c))) + (rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x17c)))) + (rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x19c)))) + rf(m, (mdl4 + 0x1ac)));
        m.set_f32(((fp - 0x44)) as u32, ((((rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x180))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x190)))) + (rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x1a0)))) + rf(m, (mdl4 + 0x1b0))).to_f32());
        m.set_f32(((fp - 0x4c)) as u32, (((rf(m, (mdl1 + 0x180)) * rf(m, (fp - 0x44))) + (rf(m, (mdl1 + 0x178)) * fVar1)) + (rf(m, (mdl1 + 0x17c)) * fVar2)).to_f32());
        m.set_f32(((fp - 0x48)) as u32, (((rf(m, (mdl1 + 0x190)) * rf(m, (fp - 0x44))) + (rf(m, (mdl1 + 0x18c)) * fVar2)) + (rf(m, (mdl1 + 0x188)) * fVar1)).to_f32());
        fVar1 = (fVar1 * rf(m, (mdl1 + 0x198)));
        fVar2 = (fVar2 * rf(m, (mdl1 + 0x19c)));
        m.set_f32(((fp - 0x44)) as u32, (rf(m, (fp - 0x44)) * rf(m, (mdl1 + 0x1a0))).to_f32());
        m.set_f32(((fp - 0x44)) as u32, ((fVar1 + fVar2) + rf(m, (fp - 0x44))).to_f32());
    } else if ri(m, (mdl1 + 0x2ec)) == 12 {
        iVar14 = ri(m, (mdl1 + 0x2bc));
        if iVar14 != 0 {
            fVar1 = ((((rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x198))) + (rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x178)))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x188)))) + rf(m, (mdl4 + 0x1a8)));
            fVar2 = ((((rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x19c))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x18c)))) + (rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x17c)))) + rf(m, (mdl4 + 0x1ac)));
            m.set_f32(((fp - 0x44)) as u32, ((((rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x180))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x190)))) + (rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x1a0)))) + rf(m, (mdl4 + 0x1b0))).to_f32());
            m.set_f32(((fp - 0x4c)) as u32, (((rf(m, (iVar14 + 0x180)) * rf(m, (fp - 0x44))) + (rf(m, (iVar14 + 0x17c)) * fVar2)) + (rf(m, (iVar14 + 0x178)) * fVar1)).to_f32());
            m.set_f32(((fp - 0x48)) as u32, (((rf(m, (iVar14 + 0x190)) * rf(m, (fp - 0x44))) + (rf(m, (iVar14 + 0x18c)) * fVar2)) + (rf(m, (iVar14 + 0x188)) * fVar1)).to_f32());
            fVar1 = (fVar1 * rf(m, (iVar14 + 0x198)));
            fVar2 = (fVar2 * rf(m, (iVar14 + 0x19c)));
            m.set_f32(((fp - 0x44)) as u32, (rf(m, (fp - 0x44)) * rf(m, (iVar14 + 0x1a0))).to_f32());
        m.set_f32(((fp - 0x44)) as u32, ((fVar1 + fVar2) + rf(m, (fp - 0x44))).to_f32());
        } else {
        fVar2 = (rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x198)));
        fVar3 = (rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x17c)));
        fVar1 = (rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x19c)));
        m.set_f32(((fp - 0x44)) as u32, ((((rf(m, (fp - 0x44)) * rf(m, (mdl4 + 0x1a0))) + (rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x180)))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x190)))) + rf(m, (mdl4 + 0x1b0))).to_f32());
        m.set_f32(((fp - 0x4c)) as u32, (((fVar2 + (rf(m, (fp - 0x4c)) * rf(m, (mdl4 + 0x178)))) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x188)))) + rf(m, (mdl4 + 0x1a8))).to_f32());
        m.set_f32(((fp - 0x48)) as u32, (((fVar1 + fVar3) + (rf(m, (fp - 0x48)) * rf(m, (mdl4 + 0x18c)))) + rf(m, (mdl4 + 0x1ac))).to_f32());
        }
    }
    m.set_f32(((soc + 0x1c)) as u32, (((rf(m, (iVar4 + 0x228)) * rf(m, (fp - 0x44))) + (rf(m, (iVar4 + 0x220)) * rf(m, (fp - 0x4c)))) + (rf(m, (iVar4 + 0x224)) * rf(m, (fp - 0x48)))).to_f32());
    m.set_f32(((soc + 0x20)) as u32, (((rf(m, (iVar4 + 0x234)) * rf(m, (fp - 0x44))) + (rf(m, (iVar4 + 0x230)) * rf(m, (fp - 0x48)))) + (rf(m, (iVar4 + 0x22c)) * rf(m, (fp - 0x4c)))).to_f32());
    m.set_f32(((soc + 0x24)) as u32, (((rf(m, (fp - 0x44)) * rf(m, (iVar4 + 0x240))) + (rf(m, (fp - 0x4c)) * rf(m, (iVar4 + 0x238)))) + (rf(m, (fp - 0x48)) * rf(m, (iVar4 + 0x23c)))).to_f32());
    lVar15 = x(surface_of_contact_compute_area(m, soc as u32));
    m.set_f32(((soc + 0x74)) as u32, lVar15.to_f32());
// LAB_084a53b8:
    return (0 < ri(m, (fp - 0x210))) as i32;
// 8 TODO marks

}

/// [soft 0x084a61b8 contactInitialSpeedArrange]: arranges the initial contact speeds of two models from the contact info.
/// The sums are written in the order and grouping of the machine code (x87), not of the decompile.
#[allow(clippy::too_many_arguments)]
pub fn contact_initial_speed_arrange(m: &mut Mem, _o: &mut dyn Outside, mdl1: u32, mdl2: u32, info: u32) {
    let fr = m.local(0x20);
    contact_initial_speed_arrange_body(m, fr + 0x20, mdl1 as i32, mdl2 as i32, info as i32);
    m.release(0x20);
}

#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::too_many_arguments)]
fn contact_initial_speed_arrange_body(m: &mut Mem, fp: u32, mdl1: i32, mdl2: i32, info: i32) {
    let fp = fp as i32;
    macro_rules! R { ($a:expr) => { rf(m, ($a) as i32) }; }
    macro_rules! W { ($a:expr, $v:expr) => {{ let v = $v; m.set_f32((($a) as i32) as u32, v.to_f32()) }}; }
    let kind = ri(m, mdl1 + 0x2ec);
    let d = ri(m, mdl2 + 0x2f0);
    let g = ri(m, info + 0x10);
    let c = ri(m, mdl1 + 0x2bc);
    let one = x(1.0f32) - x(0.0f32);
    // local slots (ebp-relative in the original): L_c, L_8, L_4, L_10, L_14, L_18, L_1c
    // block A: the contact velocity moved into the first model's frame
    if kind == 2 || kind == 12 {
        if kind == 12 && c == 0 {
            W!(fp - 0xc, ((R!(g + 0x178) * R!(d + 0x30)) + (R!(g + 0x17c) * R!(d + 0x34))) + (R!(g + 0x180) * R!(d + 0x38)));
            W!(fp - 0x8, ((R!(g + 0x188) * R!(d + 0x30)) + (R!(g + 0x18c) * R!(d + 0x34))) + (R!(g + 0x190) * R!(d + 0x38)));
            let phi4 = ((R!(d + 0x34) * R!(g + 0x19c)) + (R!(d + 0x30) * R!(g + 0x198))) + (R!(d + 0x38) * R!(g + 0x1a0));
            W!(fp - 0x4, phi4);
        } else {
            let e = if kind == 2 { mdl1 } else { c };
            let t1 = (((R!(d + 0x34) * R!(e + 0x188)) + (R!(d + 0x30) * R!(e + 0x178))) + (R!(d + 0x38) * R!(e + 0x198))) + R!(e + 0x1a8);
            W!(fp - 0xc, t1);
            let t2 = (((R!(d + 0x30) * R!(e + 0x17c)) + (R!(d + 0x34) * R!(e + 0x18c))) + (R!(d + 0x38) * R!(e + 0x19c))) + R!(e + 0x1ac);
            W!(fp - 0x8, t2);
            let phi3 = ((R!(d + 0x38) * R!(e + 0x1a0)) + ((R!(d + 0x34) * R!(e + 0x190)) + (R!(d + 0x30) * R!(e + 0x180)))) + R!(e + 0x1b0);
            W!(fp - 0x4, phi3);
            W!(fp - 0xc, ((R!(g + 0x178) * t1) + (R!(g + 0x17c) * t2)) + (R!(g + 0x180) * phi3));
            W!(fp - 0x8, ((R!(g + 0x188) * t1) + (R!(g + 0x18c) * t2)) + (R!(g + 0x190) * phi3));
            let phi4 = (phi3 * R!(g + 0x1a0)) + ((t2 * R!(g + 0x19c)) + (t1 * R!(g + 0x198)));
            W!(fp - 0x4, phi4);
        }
    }
    // block B: the contact frame
    let q = ri(m, info + 0x38);
    let (la, lb, lc) = (R!(fp - 0xc), R!(fp - 0x8), R!(fp - 0x4));
    let t5 = ((R!(q + 0x38) * lb) + (R!(q + 0x34) * la)) + (R!(q + 0x3c) * lc);
    W!(fp - 0xc, t5);
    let t6 = ((R!(q + 0x44) * la) + (R!(q + 0x48) * lb)) + (R!(q + 0x4c) * lc);
    W!(fp - 0x8, t6);
    W!(fp - 0x1c, t5 * one);
    let s13 = R!(fp - 0x1c);
    let t8 = (lc * R!(q + 0x5c)) + ((lb * R!(q + 0x58)) + (la * R!(q + 0x54)));
    W!(fp - 0x4, t8);
    W!(fp - 0xc, s13);
    let phi5 = if t6.gt(x(0.0f32)) { t6 } else { x(0.0f32) };
    W!(fp - 0x8, phi5);
    W!(fp - 0x1c, t8 * one);
    let s17 = R!(fp - 0x1c);
    W!(fp - 0x4, s17);
    let t9 = (((s13 * R!(q + 0x34)) + (phi5 * R!(q + 0x44))) + (s17 * R!(q + 0x54))) + R!(q + 0x64);
    W!(fp - 0xc, t9);
    let t10 = (((s13 * R!(q + 0x38)) + (phi5 * R!(q + 0x48))) + (s17 * R!(q + 0x58))) + R!(q + 0x68);
    W!(fp - 0x8, t10);
    let t11 = ((s17 * R!(q + 0x5c)) + ((phi5 * R!(q + 0x4c)) + (s13 * R!(q + 0x3c)))) + R!(q + 0x6c);
    W!(fp - 0x4, t11);
    // block C: the first velocity record of the second model gets the frame vector (and the first model's matrix)
    // the three outputs of the vector in the frame of g: rows 178.., 188.., 198..
    let gdot = |m: &Mem, v0: F80, v1: F80, v2: F80| -> (F80, F80, F80) {
        (
            (((v0 * rf(m, g + 0x178)) + (v1 * rf(m, g + 0x188))) + (v2 * rf(m, g + 0x198))) + rf(m, g + 0x1a8),
            (((v0 * rf(m, g + 0x17c)) + (v1 * rf(m, g + 0x18c))) + (v2 * rf(m, g + 0x19c))) + rf(m, g + 0x1ac),
            ((v2 * rf(m, g + 0x1a0)) + ((v1 * rf(m, g + 0x190)) + (v0 * rf(m, g + 0x180)))) + rf(m, g + 0x1b0),
        )
    };
    let _ = &gdot;
    if kind == 2 || (kind == 12) {
        let (n0, n1, n2) = gdot(m, t9, t10, t11);
        if kind == 12 && c == 0 {
            W!(d + 0x30, n0);
            W!(d + 0x34, n1);
            W!(d + 0x38, n2);
        } else {
            W!(d + 0x30, n0);
            W!(d + 0x34, n1);
            W!(d + 0x38, n2);
            let e = if kind == 2 { mdl1 } else { c };
            let (a, b, cc) = (R!(d + 0x30), R!(d + 0x34), R!(d + 0x38));
            W!(d + 0x30, ((R!(e + 0x17c) * b) + (R!(e + 0x178) * a)) + (R!(e + 0x180) * cc));
            W!(d + 0x34, ((R!(e + 0x188) * a) + (R!(e + 0x18c) * b)) + (R!(e + 0x190) * cc));
            W!(d + 0x38, if kind == 2 { (cc * R!(e + 0x1a0)) + ((b * R!(e + 0x19c)) + (a * R!(e + 0x198))) } else { ((b * R!(e + 0x19c)) + (a * R!(e + 0x198))) + (cc * R!(e + 0x1a0)) });
        }
    }
    // block D: the second velocity record's angular part in the frame of the second model's matrix
    let b2 = ri(m, mdl2 + 0x300);
    let (da, db, dc) = (R!(d + 0x48), R!(d + 0x4c), R!(d + 0x50));
    let t12 = ((R!(b2 + 0x1d8) * da) + (R!(b2 + 0x1dc) * db)) + (R!(b2 + 0x1e0) * dc);
    W!(fp - 0x18, t12);
    let t13 = ((R!(b2 + 0x1e4) * da) + (R!(b2 + 0x1e8) * db)) + (R!(b2 + 0x1ec) * dc);
    W!(fp - 0x14, t13);
    let t14 = (dc * R!(b2 + 0x1f8)) + ((db * R!(b2 + 0x1f4)) + (da * R!(b2 + 0x1f0)));
    W!(fp - 0x10, t14);
    if kind == 2 || (kind == 12 && c != 0) {
        let e = if kind == 2 { mdl1 } else { c };
        let t15 = (((t12 * R!(e + 0x178)) + (t13 * R!(e + 0x188))) + (t14 * R!(e + 0x198))) + R!(e + 0x1a8);
        W!(fp - 0x18, t15);
        let t16 = (((t12 * R!(e + 0x17c)) + (t13 * R!(e + 0x18c))) + (t14 * R!(e + 0x19c))) + R!(e + 0x1ac);
        W!(fp - 0x14, t16);
        let phi9 = ((t14 * R!(e + 0x1a0)) + ((t13 * R!(e + 0x190)) + (t12 * R!(e + 0x180)))) + R!(e + 0x1b0);
        W!(fp - 0x10, phi9);
        W!(fp - 0x18, ((R!(g + 0x178) * t15) + (R!(g + 0x17c) * t16)) + (R!(g + 0x180) * phi9));
        W!(fp - 0x14, ((R!(g + 0x188) * t15) + (R!(g + 0x18c) * t16)) + (R!(g + 0x190) * phi9));
        let phi10 = phi9 * R!(g + 0x1a0);
        let phi11 = (t16 * R!(g + 0x19c)) + (t15 * R!(g + 0x198));
        W!(fp - 0x10, phi10 + phi11);
    } else if kind == 12 {
        W!(fp - 0x18, ((R!(g + 0x178) * t12) + (R!(g + 0x17c) * t13)) + (R!(g + 0x180) * t14));
        W!(fp - 0x14, ((R!(g + 0x188) * t12) + (R!(g + 0x18c) * t13)) + (R!(g + 0x190) * t14));
        let phi10 = t14 * R!(g + 0x1a0);
        let phi11 = (t13 * R!(g + 0x19c)) + (t12 * R!(g + 0x198));
        W!(fp - 0x10, phi10 + phi11);
    }
    // block E: the frame vector of the contact, scaled and cut by the dynamics level of the first model
    let (la, lb, lc) = (R!(fp - 0x18), R!(fp - 0x14), R!(fp - 0x10));
    let t19 = ((R!(q + 0x38) * lb) + (R!(q + 0x34) * la)) + (R!(q + 0x3c) * lc);
    W!(fp - 0x18, t19);
    W!(fp - 0x14, ((R!(q + 0x44) * la) + (R!(q + 0x48) * lb)) + (R!(q + 0x4c) * lc));
    let t20 = (lc * R!(q + 0x5c)) + ((lb * R!(q + 0x58)) + (la * R!(q + 0x54)));
    W!(fp - 0x10, t20);
    let level = ri(m, ri(m, mdl1 + 0x304) + 0x54);
    if level == 0 {
        W!(fp - 0x10, t20 * one);
        W!(fp - 0x18, t19 * one);
    } else if level >= 0 && level <= 2 {
        W!(fp - 0x18, x(0.0f32));
        W!(fp - 0x10, x(0.0f32));
    }
    let (la, lb, lc) = (R!(fp - 0x18), R!(fp - 0x14), R!(fp - 0x10));
    let t22 = (((lb * R!(q + 0x44)) + (la * R!(q + 0x34))) + (lc * R!(q + 0x54))) + R!(q + 0x64);
    W!(fp - 0x18, t22);
    let t23 = (((la * R!(q + 0x38)) + (lb * R!(q + 0x48))) + (lc * R!(q + 0x58))) + R!(q + 0x68);
    W!(fp - 0x14, t23);
    let t24 = ((lc * R!(q + 0x5c)) + ((lb * R!(q + 0x4c)) + (la * R!(q + 0x3c)))) + R!(q + 0x6c);
    W!(fp - 0x10, t24);
    if kind == 2 || kind == 12 {
        let n0 = (((t22 * R!(g + 0x178)) + (t23 * R!(g + 0x188))) + (t24 * R!(g + 0x198))) + R!(g + 0x1a8);
        let n1 = (((t22 * R!(g + 0x17c)) + (t23 * R!(g + 0x18c))) + (t24 * R!(g + 0x19c))) + R!(g + 0x1ac);
        let n2 = ((t24 * R!(g + 0x1a0)) + ((t23 * R!(g + 0x190)) + (t22 * R!(g + 0x180)))) + R!(g + 0x1b0);
        W!(d + 0x48, n0);
        W!(d + 0x4c, n1);
        W!(d + 0x50, n2);
        if kind == 2 || c != 0 {
            let e = if kind == 2 { mdl1 } else { c };
            let (a, b, cc) = (R!(d + 0x48), R!(d + 0x4c), R!(d + 0x50));
            W!(d + 0x48, ((R!(e + 0x17c) * b) + (R!(e + 0x178) * a)) + (R!(e + 0x180) * cc));
            W!(d + 0x4c, ((R!(e + 0x188) * a) + (R!(e + 0x18c) * b)) + (R!(e + 0x190) * cc));
            W!(d + 0x50, if kind == 2 { (cc * R!(e + 0x1a0)) + ((b * R!(e + 0x19c)) + (a * R!(e + 0x198))) } else { ((b * R!(e + 0x19c)) + (a * R!(e + 0x198))) + (cc * R!(e + 0x1a0)) });
        }
    }
    let b2 = ri(m, mdl2 + 0x300);
    let (la, lb, lc) = (R!(fp - 0x18), R!(fp - 0x14), R!(fp - 0x10));
    let d = ri(m, mdl2 + 0x2f0);
    W!(d + 0x48, ((R!(b2 + 0x1dc) * lb) + (R!(b2 + 0x1d8) * la)) + (R!(b2 + 0x1e0) * lc));
    W!(d + 0x4c, ((R!(b2 + 0x1e4) * la) + (R!(b2 + 0x1e8) * lb)) + (R!(b2 + 0x1ec) * lc));
    W!(d + 0x50, ((la * R!(b2 + 0x1f0)) + (lb * R!(b2 + 0x1f4))) + (lc * R!(b2 + 0x1f8)));
}

const MDL4_BBOX: u32 = 0x086d7200;
const MDL4_BSPH: u32 = 0x086d7204;
const MDL4_OBJBOX: u32 = 0x086d7208;
const MDL4_PLANE: u32 = 0x086d720c;
const ACTOR_COLLISION_MODE: u32 = 0x086d7142;

/// [soft 0x084a5818 booleanCollisionTest]: tests two bodies by a boolean of their meshes. The mesh
/// routines it calls are hooks; the early exit (a model with no mesh) is the part that can be run
/// against the original, the part after the boolean is written from the code.
#[allow(clippy::too_many_arguments)]
pub fn boolean_collision_test(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32, p3: u32, shape_level: i32, short_: i16, soc: u32) -> i32 {
    let fr = m.local(0x17c);
    let r = boolean_collision_test_body(m, o, fr + 0x17c, p1 as i32, p2 as i32, p3 as i32, shape_level, short_, soc as i32);
    m.release(0x17c);
    r
}

#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::too_many_arguments)]
fn boolean_collision_test_body(m: &mut Mem, o: &mut dyn Outside, fp: u32, p1: i32, p2: i32, p3: i32, shape_level: i32, short_: i16, soc: i32) -> i32 {
    let fp = fp as i32;
    macro_rules! R { ($a:expr) => { rf(m, ($a) as i32) }; }
    macro_rules! W { ($a:expr, $v:expr) => {{ let v = $v; m.set_f32((($a) as i32) as u32, v.to_f32()) }}; }
    macro_rules! I32 { ($a:expr) => { ri(m, ($a) as i32) }; }
    macro_rules! S32 { ($a:expr, $v:expr) => {{ let v: i32 = $v; m.set_i32((($a) as i32) as u32, v) }}; }
    macro_rules! S16 { ($a:expr, $v:expr) => {{ let v: i16 = $v; m.set_i16((($a) as i32) as u32, v) }}; }
    // the locals, by their offset from the frame pointer in the machine code
    const L_15C: i32 = -0x15c; // the first body, or the second when that is none
    const L_158: i32 = -0x158;
    const L_154: i32 = -0x154;
    const L_150: i32 = -0x150;
    const L_14E: i32 = -0x14e;
    m.set_i16(ACTOR_COLLISION_MODE, 1);
    S16!(fp - 0x14a, short_);
    S32!(fp + L_15C, if p1 == 0 { p2 } else { p1 });
    let sv = m.i16((soc + 0x78) as u32);
    if sv == 0 {
        model4_bool_prepare(m, o, MDL4_BSPH, p3 as u32, 0, (fp - 0x118) as u32);
        S32!(fp + L_158, m.i32(MDL4_BSPH));
        S16!(fp + L_150, 0);
        S32!(fp + L_154, p1);
        S16!(fp + L_14E, 1);
        S32!(fp - 0x98, 2);
    } else if sv == -1 {
        model4_bool_prepare(m, o, MDL4_PLANE, p3 as u32, -1, (fp - 0x10c) as u32);
        S32!(fp + L_154, m.i32(MDL4_PLANE));
        S16!(fp + L_14E, 0);
        S32!(fp + L_158, p1);
        S16!(fp + L_150, 1);
        if p1 == 0 || shape_level == 1 {
            let c = I32!(fp + L_158);
            S32!(fp + L_15C, if c == 0 { p2 } else { c });
            model4_bool_prepare(m, o, MDL4_OBJBOX, I32!(fp + L_15C) as u32, 1, (fp - 0x118) as u32);
            S32!(fp + L_158, m.i32(MDL4_OBJBOX));
            S16!(fp + L_150, 0);
        }
        S32!(fp - 0x98, 3);
    } else if sv == 1 || sv == 2 {
        if sv == 1 {
            model4_bool_prepare(m, o, MDL4_BBOX, p3 as u32, 1, (fp - 0x118) as u32);
            S32!(fp + L_158, m.i32(MDL4_BBOX));
            S16!(fp + L_150, 0);
            S32!(fp + L_154, p1);
            S16!(fp + L_14E, 1);
            if p1 == 0 || shape_level == 1 {
                let c = I32!(fp + L_154);
                S32!(fp + L_15C, if c == 0 { p2 } else { c });
                model4_bool_prepare(m, o, MDL4_OBJBOX, I32!(fp + L_15C) as u32, 1, (fp - 0x118) as u32);
                S32!(fp + L_154, m.i32(MDL4_OBJBOX));
                S16!(fp + L_14E, 0);
            }
        } else {
            let c = I32!(fp + L_15C);
            S32!(fp + L_158, p3);
            S32!(fp + L_154, c);
            S16!(fp + L_150, 1);
            S16!(fp + L_14E, 1);
        }
        S32!(fp - 0x98, 2);
    }
    // the setup record of the boolean: kind 2, the mode word, no triangulation yet
    S32!(fp - 0x9c, 2);
    S32!(fp - 0x94, 0);
    S16!(fp - 0x90, 0);
    S16!(fp - 0x8e, 0);
    S16!(fp - 0x8c, 0);
    S32!(fp - 0x8, 0);
    S32!(fp - 0x4, 0);
    let b154 = I32!(fp + L_154);
    S32!(fp - 0x164, b154 + 0x1c4);
    o.si_matrix_inverse(m, (b154 + 0x1c4) as u32, (fp - 0xdc) as u32);
    let b158 = I32!(fp + L_158);
    o.si_matrix_multiply(m, (b158 + 0x1c4) as u32, (fp - 0xdc) as u32, (fp - 0x88) as u32);
    o.si_matrix_transpose_inverse(m, (fp - 0x88) as u32, (fp - 0x48) as u32);
    let a = o.si_model_convert_to_mesh(m, I32!(fp + L_154) as u32) as i32;
    S32!(fp - 0x144, a);
    S32!(fp - 0x168, I32!(fp - 0x164));
    S32!(fp - 0x16c, b158 + 0x1c4);
    if a == 0 {
        return 0;
    }
    let b = o.si_model_convert_to_mesh(m, I32!(fp + L_158) as u32) as i32;
    S32!(fp - 0x148, b);
    if b == 0 {
        return 0;
    }
    if m.i16((fp - 0x14e) as u32) != 0 {
        let mesh = I32!(fp - 0x144) as u32;
        if o.si_mesh_planar(m, mesh) as i16 == 0 {
            o.si_mesh_triangulate(m, I32!(fp - 0x144) as u32, 1);
            S16!(fp - 0x90, 1);
            S16!(fp - 0x8e, 1);
        }
    }
    if m.i16((fp - 0x150) as u32) != 0 {
        let mesh = I32!(fp - 0x148) as u32;
        if o.si_mesh_planar(m, mesh) as i16 == 0 {
            o.si_mesh_triangulate(m, I32!(fp - 0x148) as u32, 1);
            S16!(fp - 0x90, 1);
            S16!(fp - 0x8e, 1);
        }
    }
    o.si_compute_boolean(m, (fp - 0x9c) as u32, I32!(fp - 0x144) as u32, I32!(fp - 0x148) as u32, (fp - 0x140) as u32, (fp - 0x13c) as u32);
    // the meshes that came from solid models lose their faceted flag when only one of the two has it
    let a_bit = m.u8((I32!(fp - 0x144) + 0x20) as u32) & 2 != 0;
    if a_bit {
        if m.u8((I32!(fp - 0x148) + 0x20) as u32) & 2 == 0 {
            o.si_remove_faceted(m, I32!(fp - 0x144) as u32);
        }
    } else if m.u8((I32!(fp - 0x148) + 0x20) as u32) & 2 != 0 {
        o.si_remove_faceted(m, I32!(fp - 0x148) as u32);
    }
    o.si_mesh_dispose(m, (fp - 0x144) as u32);
    o.si_mesh_dispose(m, (fp - 0x148) as u32);
    let hit = (I32!(fp - 0x140) != 0) as i16;
    S16!(fp - 0x14c, hit);
    let res = I32!(fp - 0x140);
    if m.i16((fp - 0x14a) as u32) != 0 && hit != 0 && res != 0 && ri(m, res + 0x14) > 0 {
        let c = I32!(fp + L_15C);
        S32!(fp - 0x160, c);
        if ri(m, c + 0x2f4) == 0 {
            S32!(fp - 0x160, p2);
        }
        W!(fp - 0xec, x(0.0f32));
        W!(fp - 0xf0, x(0.0f32));
        W!(fp - 0xf4, x(0.0f32));
        let lv = m.i16((soc + 0x78) as u32);
        if lv == -1 {
            // the lowest vertex of the result, then every vertex within 0.1 of it goes through the second body's matrix
            let verts = ri(m, res);
            W!(fp - 0xf4, R!(verts));
            W!(fp - 0xf0, R!(verts + 4));
            W!(fp - 0xec, R!(verts + 8));
            let mut i = 0;
            while i < ri(m, res + 0x14) {
                let v = ri(m, res) + i * 0x14;
                if rf(m, fp - 0xf0).gt(R!(v + 4)) {
                    W!(fp - 0xf4, R!(v));
                    W!(fp - 0xf0, R!(v + 4));
                    W!(fp - 0xec, R!(v + 8));
                }
                i += 1;
            }
            let mut i = 0;
            let resb = I32!(fp - 0x140);
            while i < ri(m, resb + 0x14) {
                let v = ri(m, resb) + i * 0x14;
                if (R!(v + 4) - R!(fp - 0xf0)).lt(x(0.1f64)) {
                    W!(fp - 0x100, R!(v));
                    W!(fp - 0xfc, R!(v + 4) + R!(fp - 0x108));
                    W!(fp - 0xf8, R!(v + 8));
                    o.si_matrix_inverse(m, I32!(fp - 0x16c) as u32, (fp - 0xdc) as u32);
                    o.si_matrix_multiply(m, I32!(fp - 0x168) as u32, (fp - 0xdc) as u32, (fp - 0x88) as u32);
                    let (x0, y0, z0) = (R!(fp - 0x100), R!(fp - 0xfc), R!(fp - 0xf8));
                    W!(fp - 0x100, (((y0 * R!(fp - 0x78)) + (x0 * R!(fp - 0x88))) + (z0 * R!(fp - 0x68))) + R!(fp - 0x58));
                    W!(fp - 0xf8, ((z0 * R!(fp - 0x60)) + ((y0 * R!(fp - 0x70)) + (x0 * R!(fp - 0x80)))) + R!(fp - 0x50));
                    W!(fp - 0xfc, (((y0 * R!(fp - 0x74)) + (x0 * R!(fp - 0x84))) + (z0 * R!(fp - 0x64))) + R!(fp - 0x54));
                    let (nx, ny, nz) = (m.u32((fp - 0x100) as u32), m.u32((fp - 0xfc) as u32), m.u32((fp - 0xf8) as u32));
                    let model = I32!(fp - 0x160) as u32;
                    surface_of_contact_add_vertex(m, nx, ny, nz, model, soc as u32);
                }
                i += 1;
            }
        } else if lv > -2 && lv < 3 {
            // the centre of the result
            W!(fp - 0xe0, x(0.0f32));
            W!(fp - 0xe4, x(0.0f32));
            W!(fp - 0xe8, x(0.0f32));
            let resb = I32!(fp - 0x140);
            let mut i = 0;
            while i < ri(m, resb + 0x14) {
                let v = ri(m, resb) + i * 0x14;
                W!(fp - 0xe8, R!(fp - 0xe8) + R!(v));
                W!(fp - 0xe4, R!(fp - 0xe4) + R!(v + 4));
                W!(fp - 0xe0, R!(fp - 0xe0) + R!(v + 8));
                i += 1;
            }
            let n = ri(m, resb + 0x14);
            S32!(fp - 0x138, n);
            W!(fp - 0xf4, (x(1.0f32) / x(I32!(fp - 0x138))) * R!(fp - 0xe8));
            W!(fp - 0xf0, (x(1.0f32) / x(ri(m, resb + 0x14))) * R!(fp - 0xe4));
            W!(fp - 0xec, (x(1.0f32) / x(ri(m, resb + 0x14))) * R!(fp - 0xe0));
            let (nx, ny, nz) = (m.u32((fp - 0xf4) as u32), m.u32((fp - 0xf0) as u32), m.u32((fp - 0xec) as u32));
            let model = I32!(fp - 0x160) as u32;
            surface_of_contact_add_vertex(m, nx, ny, nz, model, soc as u32);
        }
    }
    o.si_mesh_dispose(m, (fp - 0x140) as u32);
    o.si_mesh_dispose(m, (fp - 0x13c) as u32);
    m.set_i16(ACTOR_COLLISION_MODE, 0);
    m.i16((fp - 0x14c) as u32) as i32 & 0xffff
}

/// [soft 0x084a5fcc detectDynaNodeCollision]: the collision test of one node pair, by the shape level of
/// the dynamics record, then down the node's two child links. `eax_in` is the register argument the
/// original takes; it only matters when `kind` lies outside 0..=3.
#[allow(clippy::too_many_arguments)]
pub fn detect_dyna_node_collision(m: &mut Mem, o: &mut dyn Outside, eax_in: u32, p2: u32, p3: u32, p4: u32, p5: u32, kind: i32, level: i32, short_: i16, soc: u32) -> i32 {
    let mut s: i16 = 0;
    let mut a: i16 = 0;
    let mut b: i16 = 0;
    let mut dynrec = eax_in;
    if kind >= 0 {
        if kind < 2 {
            dynrec = m.u32(p4 + 0x2f4);
        } else if kind < 4 {
            dynrec = m.u32(p3 + 0x2f4);
        }
    }
    if dynrec != 0 {
        let mut l = level;
        let v = m.i32(dynrec + 0x80);
        if v != -2 {
            l = v;
        }
        s = detect_dyna_bsphere_collision(m, o, p2, p3, p4, p5, (l == 0) as i16, soc);
        if l > 0 && s != 0 {
            if l == 1 {
                let w = m.i16(soc + 0x78);
                if w == 0 {
                    s = detect_dyna_bbox_bsph_collision(m, o, p2, p3, p4, p5, 1, soc);
                } else if w < 1 {
                    if w == -1 {
                        s = detect_dyna_bbox_collision(m, o, p2, p3, p4, p5, 1, soc) as i16;
                    }
                } else if w < 3 {
                    s = boolean_collision_test(m, o, p3, p4, p5, 1, 1, soc) as i16;
                }
            }
            if l > 1 && s != 0 && kind != 0 {
                let k = m.i32(p3 + 0x30);
                if k == 1 || k == 4 || k == 10 {
                    s = boolean_collision_test(m, o, p3, p4, p5, l, 1, soc) as i16;
                }
            }
        }
    }
    if short_ != 0 {
        let c1 = m.u32(p3 + 0x2c4);
        if c1 != 0 {
            a = detect_dyna_node_collision(m, o, p4, p2, c1, p4, p5, kind, level, short_, soc) as i16;
        }
        let c0 = m.u32(p3 + 0x2c0);
        if c0 != 0 {
            b = detect_dyna_node_collision(m, o, p4, p2, c0, p4, p5, kind, level, short_, soc) as i16;
        }
    }
    (s != 0 || a != 0 || b != 0) as i32
}

/// [soft 0x084a6f2c si_dynaDetectCollision]: for each collision entry of the second model's list, runs the
/// node test and moves the old hit flag back one step.
pub fn si_dyna_detect_collision(m: &mut Mem, o: &mut dyn Outside, model1: u32, model2: u32) {
    let mut e = m.u32(model2 + 0x2fc);
    while e != 0 {
        if m.i32(e + 4) == 9 {
            let p5 = m.u32(e + 0x10);
            if p5 != 0 {
                let (p3, short_, mut kind);
                if m.i32(model2 + 0x2ec) == 12 {
                    p3 = model2;
                    short_ = 1i16;
                    kind = 3;
                } else {
                    let child = m.u32(m.u32(model2 + 0x2c4) + 0x2c0);
                    if child == 0 || m.i32(model2 + 0x31c) != 1 {
                        p3 = 0;
                        short_ = 0;
                        kind = 0;
                    } else {
                        p3 = child;
                        short_ = 1;
                        kind = 1;
                        if m.u32(child + 0x2f4) != 0 {
                            kind = 2;
                        }
                    }
                }
                let soc = m.u32(e + 0x38);
                m.set_u32(soc, 0);
                m.set_i32(soc + 0x94, kind);
                m.set_u32(soc + 0x74, 0);
                let level = m.i32(m.u32(model1 + 0x304) + 0x54);
                let r = detect_dyna_node_collision(m, o, model2, model1, p3, model2, p5, kind, level, short_, soc);
                let prev = m.u16(e + 0xa);
                m.set_i16(e + 0xc, prev as i16);
                m.set_i16(e + 0xa, 0);
                if (r as i16) != 0 {
                    m.set_i16(e + 0xa, 1);
                }
            }
        }
        e = m.u32(e + 0x3c);
    }
}

/// [soft 0x084a7024 si_dynaFindCollisionState]: for each collision entry of the second model's list it
/// counts the shape levels (0 sphere, 1 box, 2 boolean) of the contact models in the entry's contact set
/// (a model repeated in a row counts once), then walks the set once more to find the entry's new state:
/// level 0 may pick the contact model into the set's `+0x84` slot, level 1 asks the stability test,
/// level 2 ends the walk with no state. A body whose motion record has a speed above a limit
/// (a double constant in the binary) loses the state, and the entry's state word (`+0xa`, the previous
/// one moves to `+0xc` elsewhere) becomes 1 or the walk's result by the entry's mode word (`+0xc`).
/// The two locals the walk sets (the result and the state) are read uninitialised by the original when
/// no contact model has a level 0..2 (the port reads the same stack slots, so such cases come out
/// undefined in the check, as in the original).
pub fn si_dyna_find_collision_state(m: &mut Mem, o: &mut dyn Outside, model1: u32, model2: u32, v: [u32; 3]) {
    let fr = m.local(0x5c);
    let fp = fr + 0x5c;
    let (r38, s3a, flag) = (fp - 0x38, fp - 0x3a, fp - 0x3c);
    let mut e = m.u32(model2 + 0x2fc);
    while e != 0 {
        if m.i32(e + 4) == 9 {
            let p5 = m.u32(e + 0x10);
            if p5 != 0 {
                let mut st = m.i16(e + 0xa) as i32;
                if st != 0 {
                    if m.i16(e + 0xc) > 0 {
                        let (mut c0, mut c1, mut c2) = (0i32, 0i32, 0i32);
                        let mut prev = 0u32;
                        let soc = m.u32(e + 0x38);
                        let n = m.i32(soc);
                        let list = |m: &Mem, i: i32| m.u32(m.u32(soc + 0x80).wrapping_add((i as u32).wrapping_mul(4)));
                        let level = |m: &Mem, model: u32| {
                            let l = m.i32(m.u32(model + 0x2f4) + 0x80);
                            if l != -2 { l } else { m.i32(m.u32(model1 + 0x304) + 0x54) }
                        };
                        let mut i = 0;
                        while i < n {
                            let model = list(m, i);
                            let l = level(m, model);
                            if prev == 0 || prev != model {
                                prev = model;
                                match l {
                                    1 => c1 += 1,
                                    0 => c0 += 1,
                                    2 => c2 += 1,
                                    _ => {}
                                }
                            }
                            i += 1;
                            if i >= m.i32(soc) { break; }
                        }
                        let sum = c0 + c1 + c2;
                        let mut i = 0;
                        m.set_i16(flag, 0);
                        let mut soc = soc;
                        if i < m.i32(soc) {
                            loop {
                                let model = list(m, i);
                                let l = level(m, model);
                                match l {
                                    0 => {
                                        if m.i32(soc + 0x94) == 3 {
                                            m.set_i32(r38, 2);
                                            if m.i32(soc) > 1 { m.set_i32(r38, 3); }
                                        } else {
                                            m.set_i32(r38, 3);
                                        }
                                        m.set_i16(s3a, 0);
                                        if c0 == 1 && c1 == 0 {
                                            m.set_i16(s3a, (c2 == 0) as i16);
                                        }
                                        let mut ax = 0u16;
                                        if m.i16(s3a) != 0 {
                                            ax = (list(m, i) == model1) as u16;
                                        }
                                        m.set_i16(s3a, ax as i16);
                                        let mut take = true;
                                        if ax != 0 {
                                            if m.i16(e + 0xc) == 2 && m.u32(soc + 0x84) != list(m, i) {
                                                m.set_i16(s3a, 0);
                                            }
                                        }
                                        if m.i16(s3a) == 0 && m.u32(soc + 0x84) != 0 {
                                            take = false;
                                        }
                                        if take {
                                            let soc2 = m.u32(e + 0x38);
                                            let pick = list(m, i);
                                            m.set_u32(soc2 + 0x84, pick);
                                        }
                                        m.set_i16(flag, 1);
                                        soc = m.u32(e + 0x38);
                                    }
                                    1 => {
                                        m.set_i32(r38, 3);
                                        if m.i32(soc) <= 3 || sum > 1 {
                                            m.set_i16(s3a, 0);
                                        } else {
                                            // the vector is copied to locals and passed by value
                                            let out = fr + 0x50;
                                            let r = crate::contact::si_dyna_evaluate_contact_stability(m, model2, m.u32(e + 0x38), v[0], v[1], v[2], out);
                                            m.set_i16(s3a, r as i16);
                                        }
                                        m.set_i16(flag, 1);
                                        soc = m.u32(e + 0x38);
                                    }
                                    2 => {
                                        m.set_i16(s3a, 0);
                                        m.set_i16(flag, 1);
                                    }
                                    _ => {}
                                }
                                i += 1;
                                if i >= m.i32(soc) || m.i16(flag) != 0 { break; }
                            }
                        }
                        let rec = m.u32(p5 + 0x2f0);
                        if rec != 0 {
                            let lim = m.f64(0x0866_0c58);
                            let big = [0x30u32, 0x34, 0x38, 0x48, 0x4c, 0x50].iter().any(|&k| x(m.f32(rec + k)).abs().gt(x(lim)));
                            if big {
                                m.set_i16(s3a, 0);
                            }
                        }
                    }
                    match m.i16(e + 0xc) {
                        1 | 2 | 3 => {
                            st = 1;
                            if m.i16(s3a) != 0 {
                                st = m.i32(r38);
                            }
                        }
                        0 | 4 => st = 1,
                        _ => {}
                    }
                    if st > 1 && m.i16(e + 0xc) == 1 {
                        contact_initial_speed_arrange(m, o, model1, model2, e);
                    }
                }
                m.set_i16(e + 0xa, st as i16);
            }
        }
        e = m.u32(e + 0x3c);
    }
    m.release(0x5c);
}

/// [soft 0x084a739c resolveCollision]: the impulse of one contact point between two bodies. It builds the
/// 9 x 9 system of the contact (the velocities of the contact point of both bodies carried to world
/// space, the cross-product block of the contact point) and solves it with `resolveLinearSystemRG`; the
/// result is a force vector written to the constraint record (`info + 0x14`). Returns 0 (with the
/// vector cleared) when the system has no solution.
#[allow(clippy::too_many_arguments)]
pub fn resolve_collision(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32, p3: u32, p4: u32, p5: f32, p6: f32, p7: f32) -> i32 {
    let fr = m.local(0x27c);
    let r = resolve_collision_body(m, o, fr + 0x27c, p1 as i32, p2 as i32, p3 as i32, p4 as i32, p5, p6, p7);
    m.release(0x27c);
    r
}

#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::too_many_arguments, clippy::let_and_return)]
fn resolve_collision_body(m: &mut Mem, o: &mut dyn Outside, fp: u32, p1: i32, p2: i32, p3: i32, p4: i32, p5: f32, p6: f32, p7: f32) -> i32 {
    let fp = fp as i32;
    macro_rules! W { ($a:expr, $v:expr) => {{ let v = $v; m.set_f32((($a) as i32) as u32, v.to_f32()) }}; }
    let (p5, p6, p7) = (x(p5), x(p6), x(p7));
    let dyn2 = ri(m, p2 + 0x2f4);
    let g = ri(m, p4 + 0x10);
    let ecx = ri(m, p1 + 0x304);
    let d2 = ri(m, p2 + 0x2f0);
    let d1 = ri(m, g + 0x2f0);
    let k1 = ri(m, p1 + 0x2ec);
    let c = ri(m, p1 + 0x2bc);
    let h = ri(m, p4 + 0x38);
    let src;
    if ri(m, p2 + 0x2ec) == 2 {
        let w = m.u32((dyn2 + 8) as u32);
        m.set_u32((fp - 0x25c) as u32, w);
        for (k, d) in [(0x20, 0xa8), (0x24, 0xa4), (0x28, 0xa0)] {
            let w = m.u32((dyn2 + k) as u32);
            m.set_u32((fp - d) as u32, w);
        }
        src = dyn2 + 0x50;
    } else {
        let w = m.u32((ecx + 0x60) as u32);
        m.set_u32((fp - 0x25c) as u32, w);
        for (k, d) in [(0x64, 0xa8), (0x68, 0xa4), (0x6c, 0xa0)] {
            let w = m.u32((ecx + k) as u32);
            m.set_u32((fp - d) as u32, w);
        }
        src = ecx + 0x94;
    }
    o.si_matrix3_copy(m, src as u32, (fp - 0x234) as u32);
    // the contact point relative to the body, carried into the frame of the first body and back
    let t1 = (((rf(m, p3 + 0x1dc) * (rf(m, p4 + 0x24) - rf(m, fp - 0xa4))) + (rf(m, p3 + 0x1d8) * (rf(m, p4 + 0x20) - rf(m, fp - 0xa8)))) + (rf(m, p3 + 0x1e0) * (rf(m, p4 + 0x28) - rf(m, fp - 0xa0))));
        W!(fp - 0x60, t1); // S11
        let t2 = (((rf(m, p3 + 0x1e4) * (rf(m, p4 + 0x20) - rf(m, fp - 0xa8))) + (rf(m, p3 + 0x1e8) * (rf(m, p4 + 0x24) - rf(m, fp - 0xa4)))) + (rf(m, p3 + 0x1ec) * (rf(m, p4 + 0x28) - rf(m, fp - 0xa0))));
        W!(fp - 0x5c, t2); // S12
        let t3 = (((rf(m, p4 + 0x28) - rf(m, fp - 0xa0)) * rf(m, p3 + 0x1f8)) + (((rf(m, p4 + 0x24) - rf(m, fp - 0xa4)) * rf(m, p3 + 0x1f4)) + ((rf(m, p4 + 0x20) - rf(m, fp - 0xa8)) * rf(m, p3 + 0x1f0))));
        W!(fp - 0x58, t3); // S13
    let gate = k1 == 2 || (k1 == 0xc && c != 0);
    if gate {
        let b = if k1 == 2 { p1 } else { c };
        let t6 = ((((t1 * rf(m, b + 0x178)) + (t2 * rf(m, b + 0x188))) + (t3 * rf(m, b + 0x198))) + rf(m, b + 0x1a8));
        W!(fp - 0xc, t6); // S18
        let t7 = ((((t1 * rf(m, b + 0x17c)) + (t2 * rf(m, b + 0x18c))) + (t3 * rf(m, b + 0x19c))) + rf(m, b + 0x1ac));
        W!(fp - 0x8, t7); // S19
        let phi3 = (((t3 * rf(m, b + 0x1a0)) + ((t2 * rf(m, b + 0x190)) + (t1 * rf(m, b + 0x180)))) + rf(m, b + 0x1b0));
        W!(fp - 0x4, phi3);
        let (phi2, phi1) = (t6, t7);
        W!(fp - 0xc, (((rf(m, g + 0x178) * phi2) + (rf(m, g + 0x17c) * phi1)) + (rf(m, g + 0x180) * phi3))); // S21
        W!(fp - 0x8, (((rf(m, g + 0x188) * phi2) + (rf(m, g + 0x18c) * phi1)) + (rf(m, g + 0x190) * phi3))); // S22
        let phi4 = (phi3 * rf(m, g + 0x1a0));
        let phi5 = ((phi1 * rf(m, g + 0x19c)) + (phi2 * rf(m, g + 0x198)));
        W!(fp - 0x4, (phi4 + phi5)); // S23
    } else if k1 == 0xc {
        W!(fp - 0xc, (((rf(m, g + 0x178) * t1) + (rf(m, g + 0x17c) * t2)) + (rf(m, g + 0x180) * t3))); // S14
        W!(fp - 0x8, (((rf(m, g + 0x188) * t1) + (rf(m, g + 0x18c) * t2)) + (rf(m, g + 0x190) * t3))); // S15
        let phi4 = t3 * rf(m, g + 0x1a0);
        let phi5 = (t2 * rf(m, g + 0x19c)) + (t1 * rf(m, g + 0x198));
        W!(fp - 0x4, (phi4 + phi5)); // S23
    }
    let (x0, y0, z0) = (rf(m, fp - 0xc), rf(m, fp - 0x8), rf(m, fp - 0x4));
    W!(fp - 0xc, (((rf(m, h + 0x38) * y0) + (rf(m, h + 0x34) * x0)) + (rf(m, h + 0x3c) * z0))); // S24
        W!(fp - 0x8, (((rf(m, h + 0x44) * x0) + (rf(m, h + 0x48) * y0)) + (rf(m, h + 0x4c) * z0))); // S25
        W!(fp - 0x4, (((y0 * rf(m, h + 0x58)) + (x0 * rf(m, h + 0x54))) + (z0 * rf(m, h + 0x5c)))); // S26
    W!(fp - 0x6c, (((rf(m, p3 + 0x1dc) * rf(m, fp - 0xa4)) + (rf(m, p3 + 0x1d8) * rf(m, fp - 0xa8))) + (rf(m, p3 + 0x1e0) * rf(m, fp - 0xa0)))); // S27
        W!(fp - 0x68, (((rf(m, p3 + 0x1e4) * rf(m, fp - 0xa8)) + (rf(m, p3 + 0x1e8) * rf(m, fp - 0xa4))) + (rf(m, p3 + 0x1ec) * rf(m, fp - 0xa0)))); // S28
        W!(fp - 0x64, ((rf(m, fp - 0xa0) * rf(m, p3 + 0x1f8)) + ((rf(m, fp - 0xa4) * rf(m, p3 + 0x1f4)) + (rf(m, fp - 0xa8) * rf(m, p3 + 0x1f0))))); // S29
    let (ax, ay, az) = (m.f32((fp - 0x6c) as u32), m.f32((fp - 0x68) as u32), m.f32((fp - 0x64) as u32));
    crate::settings::si_dyna_linear_speed_compute(m, p2 as u32, ax, ay, az, (fp - 0x78) as u32);
    if gate {
        let b = if k1 == 2 { p1 } else { c };
        let t10 = ((((rf(m, fp - 0x74) * rf(m, b + 0x188)) + (rf(m, fp - 0x78) * rf(m, b + 0x178))) + (rf(m, fp - 0x70) * rf(m, b + 0x198))) + rf(m, b + 0x1a8));
        W!(fp - 0x18, t10); // S37
        let t11 = ((((rf(m, fp - 0x78) * rf(m, b + 0x17c)) + (rf(m, fp - 0x74) * rf(m, b + 0x18c))) + (rf(m, fp - 0x70) * rf(m, b + 0x19c))) + rf(m, b + 0x1ac));
        W!(fp - 0x14, t11); // S38
        let phi8 = (((rf(m, fp - 0x70) * rf(m, b + 0x1a0)) + ((rf(m, fp - 0x74) * rf(m, b + 0x190)) + (rf(m, fp - 0x78) * rf(m, b + 0x180)))) + rf(m, b + 0x1b0));
        W!(fp - 0x10, phi8); // S39
        let (phi7, phi6) = (t10, t11);
        W!(fp - 0x18, (((rf(m, g + 0x178) * phi7) + (rf(m, g + 0x17c) * phi6)) + (rf(m, g + 0x180) * phi8))); // S40
        W!(fp - 0x14, (((rf(m, g + 0x188) * phi7) + (rf(m, g + 0x18c) * phi6)) + (rf(m, g + 0x190) * phi8))); // S41
        let phi9 = ((phi8 * rf(m, g + 0x1a0)) + ((phi6 * rf(m, g + 0x19c)) + (phi7 * rf(m, g + 0x198))));
        W!(fp - 0x10, phi9); // S42
    } else if k1 == 0xc {
        W!(fp - 0x18, (((rf(m, g + 0x17c) * rf(m, fp - 0x74)) + (rf(m, g + 0x178) * rf(m, fp - 0x78))) + (rf(m, g + 0x180) * rf(m, fp - 0x70)))); // S33
        W!(fp - 0x14, (((rf(m, g + 0x188) * rf(m, fp - 0x78)) + (rf(m, g + 0x18c) * rf(m, fp - 0x74))) + (rf(m, g + 0x190) * rf(m, fp - 0x70)))); // S34
        let phi9 = ((rf(m, fp - 0x74) * rf(m, g + 0x19c)) + (rf(m, fp - 0x78) * rf(m, g + 0x198))) + (rf(m, fp - 0x70) * rf(m, g + 0x1a0));
        W!(fp - 0x10, phi9); // S42
    }
    let (x1, y1, z1) = (rf(m, fp - 0x18), rf(m, fp - 0x14), rf(m, fp - 0x10));
    W!(fp - 0x260, (rf(m, h + 0x34) * x1)); // S43
        W!(fp - 0x260, ((rf(m, h + 0x38) * y1) + rf(m, fp - 0x260))); // S44
        let t12 = ((rf(m, h + 0x3c) * z1) + rf(m, fp - 0x260));
        W!(fp - 0x260, t12); // S45
        W!(fp - 0x18, t12); // S46
        let t13 = (((rf(m, h + 0x44) * x1) + (rf(m, h + 0x48) * y1)) + (rf(m, h + 0x4c) * z1));
        W!(fp - 0x14, t13); // S47
        let t14 = ((z1 * rf(m, h + 0x5c)) + ((y1 * rf(m, h + 0x58)) + (x1 * rf(m, h + 0x54))));
        W!(fp - 0x10, t14); // S48
    let first_step = m.i16(0x086d7140) != 0 && d1 != 0;
    if first_step {
        W!(fp - 0x24, (p7 * rf(m, d1 + 0x30))); // S49
        W!(fp - 0x20, (p7 * rf(m, d1 + 0x34))); // S50
        W!(fp - 0x1c, (p7 * rf(m, d1 + 0x38))); // S51
        W!(fp - 0x48, (p7 * rf(m, d1 + 0x48))); // S52
        W!(fp - 0x44, (p7 * rf(m, d1 + 0x4c))); // S53
        W!(fp - 0x40, (p7 * rf(m, d1 + 0x50))); // S54
        let t15 = ((rf(m, d1 + 0x4c) * rf(m, h + 0x30)) - (rf(m, d1 + 0x50) * rf(m, h + 0x2c)));
        W!(fp - 0x30, t15); // S55
        let t16 = ((rf(m, d1 + 0x50) * rf(m, h + 0x28)) - (rf(m, d1 + 0x48) * rf(m, h + 0x30)));
        W!(fp - 0x2c, t16); // S56
        let t17 = ((rf(m, d1 + 0x48) * rf(m, h + 0x2c)) - (rf(m, d1 + 0x4c) * rf(m, h + 0x28)));
        W!(fp - 0x28, t17); // S57
        let t18 = (t15 + rf(m, d1 + 0x30));
        W!(fp - 0x30, t18); // S58
        let t19 = (t16 + rf(m, d1 + 0x34));
        W!(fp - 0x2c, t19); // S59
        let t20 = (t17 + rf(m, d1 + 0x38));
        W!(fp - 0x28, t20); // S60
        let t21 = (((rf(m, g + 0x178) * t18) + (rf(m, g + 0x17c) * t19)) + (rf(m, g + 0x180) * t20));
        W!(fp - 0x30, t21); // S61
        let t22 = (((rf(m, g + 0x188) * t18) + (rf(m, g + 0x18c) * t19)) + (rf(m, g + 0x190) * t20));
        W!(fp - 0x2c, t22); // S62
        let t23 = ((t20 * rf(m, g + 0x1a0)) + ((t19 * rf(m, g + 0x19c)) + (t18 * rf(m, g + 0x198))));
        W!(fp - 0x28, t23); // S63
        let t24 = (((rf(m, h + 0x34) * t21) + (rf(m, h + 0x38) * t22)) + (rf(m, h + 0x3c) * t23));
        W!(fp - 0x30, t24); // S64
        let t25 = (((rf(m, h + 0x44) * t21) + (rf(m, h + 0x48) * t22)) + (rf(m, h + 0x4c) * t23));
        W!(fp - 0x2c, t25); // S65
        let t26 = (rf(m, fp - 0x260) - t24);
        W!(fp - 0x260, t26); // S66
        W!(fp - 0x18, t26); // S67
        W!(fp - 0x28, ((t23 * rf(m, h + 0x5c)) + ((t22 * rf(m, h + 0x58)) + (t21 * rf(m, h + 0x54))))); // S68
        W!(fp - 0x14, (t13 - t25)); // S69
        W!(fp - 0x10, (t14 - ((t23 * rf(m, h + 0x5c)) + ((t22 * rf(m, h + 0x58)) + (t21 * rf(m, h + 0x54)))))); // S70
    }
    let t27 = (((rf(m, p3 + 0x1dc) * rf(m, d2 + 0x4c)) + (rf(m, p3 + 0x1d8) * rf(m, d2 + 0x48))) + (rf(m, p3 + 0x1e0) * rf(m, d2 + 0x50)));
        W!(fp - 0x3c, t27); // S71
        let t28 = (((rf(m, p3 + 0x1e4) * rf(m, d2 + 0x48)) + (rf(m, p3 + 0x1e8) * rf(m, d2 + 0x4c))) + (rf(m, p3 + 0x1ec) * rf(m, d2 + 0x50)));
        W!(fp - 0x38, t28); // S72
        let t29 = ((rf(m, d2 + 0x50) * rf(m, p3 + 0x1f8)) + ((rf(m, d2 + 0x4c) * rf(m, p3 + 0x1f4)) + (rf(m, d2 + 0x48) * rf(m, p3 + 0x1f0))));
        W!(fp - 0x34, t29); // S73
    if gate {
        let b = if k1 == 2 { p1 } else { c };
        let t32 = ((((t27 * rf(m, b + 0x178)) + (t28 * rf(m, b + 0x188))) + (t29 * rf(m, b + 0x198))) + rf(m, b + 0x1a8));
        W!(fp - 0x3c, t32); // S78
        let t33 = ((((t27 * rf(m, b + 0x17c)) + (t28 * rf(m, b + 0x18c))) + (t29 * rf(m, b + 0x19c))) + rf(m, b + 0x1ac));
        W!(fp - 0x38, t33); // S79
        let phi12 = (((t29 * rf(m, b + 0x1a0)) + ((t28 * rf(m, b + 0x190)) + (t27 * rf(m, b + 0x180)))) + rf(m, b + 0x1b0));
        W!(fp - 0x34, phi12); // S80
        let (phi10, phi11) = (t32, t33);
        W!(fp - 0x3c, (((rf(m, g + 0x178) * phi10) + (rf(m, g + 0x17c) * phi11)) + (rf(m, g + 0x180) * phi12))); // S81
        W!(fp - 0x38, (((rf(m, g + 0x188) * phi10) + (rf(m, g + 0x18c) * phi11)) + (rf(m, g + 0x190) * phi12))); // S82
        let phi13 = ((phi11 * rf(m, g + 0x19c)) + (phi10 * rf(m, g + 0x198)));
        let phi14 = (phi12 * rf(m, g + 0x1a0));
        W!(fp - 0x34, (phi13 + phi14)); // S83
    } else if k1 == 0xc {
        W!(fp - 0x3c, (((rf(m, g + 0x178) * t27) + (rf(m, g + 0x17c) * t28)) + (rf(m, g + 0x180) * t29))); // S74
        W!(fp - 0x38, (((rf(m, g + 0x188) * t27) + (rf(m, g + 0x18c) * t28)) + (rf(m, g + 0x190) * t29))); // S75
        let phi13 = (t28 * rf(m, g + 0x19c)) + (t27 * rf(m, g + 0x198));
        let phi14 = t29 * rf(m, g + 0x1a0);
        W!(fp - 0x34, (phi13 + phi14)); // S83
    }
    let (x3, y3, z3) = (rf(m, fp - 0x3c), rf(m, fp - 0x38), rf(m, fp - 0x34));
    W!(fp - 0x264, (rf(m, h + 0x34) * x3)); // S84
        W!(fp - 0x264, ((rf(m, h + 0x38) * y3) + rf(m, fp - 0x264))); // S85
        let t34 = ((rf(m, h + 0x3c) * z3) + rf(m, fp - 0x264));
        W!(fp - 0x264, t34); // S86
        W!(fp - 0x3c, t34); // S87
        let t35 = (((rf(m, h + 0x44) * x3) + (rf(m, h + 0x48) * y3)) + (rf(m, h + 0x4c) * z3));
        W!(fp - 0x38, t35); // S88
        let t36 = ((z3 * rf(m, h + 0x5c)) + ((y3 * rf(m, h + 0x58)) + (x3 * rf(m, h + 0x54))));
        W!(fp - 0x34, t36); // S89
    if first_step {
        let t37 = (((rf(m, g + 0x17c) * rf(m, d1 + 0x4c)) + (rf(m, g + 0x178) * rf(m, d1 + 0x48))) + (rf(m, g + 0x180) * rf(m, d1 + 0x50)));
        W!(fp - 0x54, t37); // S90
        let t38 = (((rf(m, g + 0x188) * rf(m, d1 + 0x48)) + (rf(m, g + 0x18c) * rf(m, d1 + 0x4c))) + (rf(m, g + 0x190) * rf(m, d1 + 0x50)));
        W!(fp - 0x50, t38); // S91
        let t39 = ((rf(m, d1 + 0x50) * rf(m, g + 0x1a0)) + ((rf(m, d1 + 0x4c) * rf(m, g + 0x19c)) + (rf(m, d1 + 0x48) * rf(m, g + 0x198))));
        W!(fp - 0x4c, t39); // S92
        let t40 = (((rf(m, h + 0x34) * t37) + (rf(m, h + 0x38) * t38)) + (rf(m, h + 0x3c) * t39));
        W!(fp - 0x54, t40); // S93
        let t41 = (((rf(m, h + 0x44) * t37) + (rf(m, h + 0x48) * t38)) + (rf(m, h + 0x4c) * t39));
        W!(fp - 0x50, t41); // S94
        let t42 = (rf(m, fp - 0x264) - t40);
        W!(fp - 0x264, t42); // S95
        W!(fp - 0x3c, t42); // S96
        W!(fp - 0x38, (t35 - t41)); // S97
        W!(fp - 0x4c, ((t39 * rf(m, h + 0x5c)) + ((t38 * rf(m, h + 0x58)) + (t37 * rf(m, h + 0x54))))); // S98
        W!(fp - 0x34, (t36 - ((t39 * rf(m, h + 0x5c)) + ((t38 * rf(m, h + 0x58)) + (t37 * rf(m, h + 0x54)))))); // S99
    }
    let t43 = (((rf(m, p3 + 0x1dc) * ((rf(m, fp - 0x220) * rf(m, d2 + 0x50)) + ((rf(m, fp - 0x224) * rf(m, d2 + 0x4c)) + (rf(m, fp - 0x228) * rf(m, d2 + 0x48))))) + (rf(m, p3 + 0x1d8) * ((rf(m, fp - 0x22c) * rf(m, d2 + 0x50)) + ((rf(m, fp - 0x230) * rf(m, d2 + 0x4c)) + (rf(m, fp - 0x234) * rf(m, d2 + 0x48)))))) + (rf(m, p3 + 0x1e0) * (((rf(m, d2 + 0x4c) * rf(m, fp - 0x218)) + (rf(m, d2 + 0x48) * rf(m, fp - 0x21c))) + (rf(m, d2 + 0x50) * rf(m, fp - 0x214)))));
        W!(fp - 0x84, t43); // S100
        let t44 = (((rf(m, p3 + 0x1e4) * ((rf(m, fp - 0x22c) * rf(m, d2 + 0x50)) + ((rf(m, fp - 0x230) * rf(m, d2 + 0x4c)) + (rf(m, fp - 0x234) * rf(m, d2 + 0x48))))) + (rf(m, p3 + 0x1e8) * ((rf(m, fp - 0x220) * rf(m, d2 + 0x50)) + ((rf(m, fp - 0x224) * rf(m, d2 + 0x4c)) + (rf(m, fp - 0x228) * rf(m, d2 + 0x48)))))) + (rf(m, p3 + 0x1ec) * (((rf(m, d2 + 0x4c) * rf(m, fp - 0x218)) + (rf(m, d2 + 0x48) * rf(m, fp - 0x21c))) + (rf(m, d2 + 0x50) * rf(m, fp - 0x214)))));
        W!(fp - 0x80, t44); // S101
        let t45 = (((((rf(m, d2 + 0x4c) * rf(m, fp - 0x218)) + (rf(m, d2 + 0x48) * rf(m, fp - 0x21c))) + (rf(m, d2 + 0x50) * rf(m, fp - 0x214))) * rf(m, p3 + 0x1f8)) + ((((rf(m, fp - 0x220) * rf(m, d2 + 0x50)) + ((rf(m, fp - 0x224) * rf(m, d2 + 0x4c)) + (rf(m, fp - 0x228) * rf(m, d2 + 0x48)))) * rf(m, p3 + 0x1f4)) + (((rf(m, fp - 0x22c) * rf(m, d2 + 0x50)) + ((rf(m, fp - 0x230) * rf(m, d2 + 0x4c)) + (rf(m, fp - 0x234) * rf(m, d2 + 0x48)))) * rf(m, p3 + 0x1f0))));
        W!(fp - 0x7c, t45); // S102
    if gate {
        let b = if k1 == 2 { p1 } else { c };
        let t48 = ((((t43 * rf(m, b + 0x178)) + (t44 * rf(m, b + 0x188))) + (t45 * rf(m, b + 0x198))) + rf(m, b + 0x1a8));
        W!(fp - 0x84, t48); // S107
        let t49 = ((((t43 * rf(m, b + 0x17c)) + (t44 * rf(m, b + 0x18c))) + (t45 * rf(m, b + 0x19c))) + rf(m, b + 0x1ac));
        W!(fp - 0x80, t49); // S108
        let phi17 = (((t45 * rf(m, b + 0x1a0)) + ((t44 * rf(m, b + 0x190)) + (t43 * rf(m, b + 0x180)))) + rf(m, b + 0x1b0));
        W!(fp - 0x7c, phi17); // S109
        let (phi15, phi16) = (t48, t49);
        W!(fp - 0x84, (((rf(m, g + 0x178) * phi15) + (rf(m, g + 0x17c) * phi16)) + (rf(m, g + 0x180) * phi17))); // S110
        W!(fp - 0x80, (((rf(m, g + 0x188) * phi15) + (rf(m, g + 0x18c) * phi16)) + (rf(m, g + 0x190) * phi17))); // S111
        let phi18 = ((phi16 * rf(m, g + 0x19c)) + (phi15 * rf(m, g + 0x198)));
        let phi19 = (phi17 * rf(m, g + 0x1a0));
        W!(fp - 0x7c, (phi18 + phi19)); // S112
    } else if k1 == 0xc {
        W!(fp - 0x84, (((rf(m, g + 0x178) * t43) + (rf(m, g + 0x17c) * t44)) + (rf(m, g + 0x180) * t45))); // S103
        W!(fp - 0x80, (((rf(m, g + 0x188) * t43) + (rf(m, g + 0x18c) * t44)) + (rf(m, g + 0x190) * t45))); // S104
        let phi18 = (t44 * rf(m, g + 0x19c)) + (t43 * rf(m, g + 0x198));
        let phi19 = t45 * rf(m, g + 0x1a0);
        W!(fp - 0x7c, (phi18 + phi19)); // S112
    }
    let (x4, y4, z4) = (rf(m, fp - 0x84), rf(m, fp - 0x80), rf(m, fp - 0x7c));
    let t50 = (((rf(m, h + 0x38) * y4) + (rf(m, h + 0x34) * x4)) + (rf(m, h + 0x3c) * z4));
        W!(fp - 0x84, t50); // S113
        let t51 = (((rf(m, h + 0x44) * x4) + (rf(m, h + 0x48) * y4)) + (rf(m, h + 0x4c) * z4));
        W!(fp - 0x80, t51); // S114
        let t52 = ((z4 * rf(m, h + 0x5c)) + ((y4 * rf(m, h + 0x58)) + (x4 * rf(m, h + 0x54))));
        W!(fp - 0x7c, t52); // S115
    if first_step {
        W!(fp - 0x84, (t50 - rf(m, fp - 0x54))); // S116
        W!(fp - 0x80, (t51 - rf(m, fp - 0x50))); // S117
        W!(fp - 0x7c, (t52 - rf(m, fp - 0x4c))); // S118
    }
    crate::dynamic::si_vector_clean(m, (fp - 0xc) as u32, 0);
    crate::dynamic::si_vector_clean(m, (fp - 0x18) as u32, 2);
    crate::dynamic::si_vector_clean(m, (fp - 0x3c) as u32, 2);
    crate::dynamic::si_vector_clean(m, (fp - 0x84) as u32, 2);
    // the system: a 9 x 10 matrix of floats from fp - 0x210, cleared and then filled with the contact terms
    W!(fp - 0x268, (-(p6))); // S120
    for i in 0..9i32 {
        for k in 0..10i32 {
            m.set_u32((fp - 0x1ec + 40 * i - 4 * k) as u32, 0);
        }
    }
    for a in [0x198, 0x16c, 0x140, 0x204, 0x1d8, 0x1ac] {
        m.set_u32((fp - a) as u32, 0x3f800000);
    }
    W!(fp - 0x18c, rf(m, fp - 0x25c)); // S123
        W!(fp - 0x174, (rf(m, fp - 0x25c) * rf(m, fp - 0x18))); // S126
        W!(fp - 0x160, rf(m, fp - 0x25c)); // S127
        W!(fp - 0x118, rf(m, fp - 0x8)); // S128
        W!(fp - 0xf8, rf(m, fp - 0x4)); // S129
        W!(fp - 0x14c, (rf(m, fp - 0x25c) * rf(m, fp - 0x14))); // S130
        W!(fp - 0x134, rf(m, fp - 0x25c)); // S131
        let t53 = (-(rf(m, fp - 0x4)));
        W!(fp - 0x11c, t53); // S132
        let t54 = (-(rf(m, fp - 0xc)));
        W!(fp - 0xf0, t54); // S133
        W!(fp - 0x124, (rf(m, fp - 0x25c) * rf(m, fp - 0x10))); // S134
        W!(fp - 0x108, rf(m, fp - 0x234)); // S135
        W!(fp - 0x104, rf(m, fp - 0x230)); // S136
        W!(fp - 0x100, rf(m, fp - 0x22c)); // S137
        W!(fp - 0xfc, rf(m, fp - 0x84)); // S138
        W!(fp - 0xe0, rf(m, fp - 0x228)); // S139
        W!(fp - 0xdc, rf(m, fp - 0x224)); // S140
        W!(fp - 0xd8, rf(m, fp - 0x220)); // S141
        W!(fp - 0xd4, rf(m, fp - 0x80)); // S142
        let t55 = (-(rf(m, fp - 0x8)));
        W!(fp - 0xd0, t55); // S143
        W!(fp - 0xcc, rf(m, fp - 0xc)); // S144
        W!(fp - 0xb8, rf(m, fp - 0x21c)); // S145
        W!(fp - 0xb4, rf(m, fp - 0x218)); // S146
        W!(fp - 0x1c8, rf(m, fp - 0xc)); // S147
        W!(fp - 0x1f4, rf(m, fp - 0x4)); // S148
        W!(fp - 0xb0, rf(m, fp - 0x214)); // S149
        W!(fp - 0xac, rf(m, fp - 0x7c)); // S150
        W!(fp - 0x1f0, t55); // S152
        W!(fp - 0x1d0, t53); // S154
        W!(fp - 0x1a8, rf(m, fp - 0x8)); // S156
        W!(fp - 0x1a4, t54); // S157
        let t56 = ((((rf(m, fp - 0xc) * rf(m, fp - 0x34)) + rf(m, fp - 0x14)) - (rf(m, fp - 0x4) * rf(m, fp - 0x3c))) * rf(m, fp - 0x268));
        W!(fp - 0x268, t56); // S158
        W!(fp - 0x1c4, t56); // S159
    let r = resolve_linear_system_rg(m, (fp - 0x210) as u32, 9, 10, 9);
    if (r as i16) == 0 {
        m.set_u32((p4 + 0x1c) as u32, 0);
        m.set_u32((p4 + 0x18) as u32, 0);
        m.set_u32((p4 + 0x14) as u32, 0);
        return 0;
    }
    let t58 = (-(rf(m, fp - 0x1c4)));
        W!(fp - 0x8c, t58); // S160
        let t57 = (-(p5));
        let t59 = (t57 * rf(m, fp - 0x19c));
        W!(fp - 0x88, t59); // S161
        let t60 = (t57 * rf(m, fp - 0x1ec));
        W!(fp - 0x90, t60); // S162
    let t_dot = ((t58 * rf(m, 0x086dc1fc)) + (t60 * rf(m, 0x086dc1f8))) + (t59 * rf(m, 0x086dc200));
    if t_dot.lt(x(0.0f32)) {
        m.set_u32((fp - 0x88) as u32, 0);
        m.set_u32((fp - 0x8c) as u32, 0);
        m.set_u32((fp - 0x90) as u32, 0);
    }
    // the force carried back through the constraint's frame and scaled by the reciprocal of the time step
    let (va, vb, vc) = (rf(m, fp - 0x90), rf(m, fp - 0x8c), rf(m, fp - 0x88));
    let t61 = (((vb * rf(m, h + 0x44)) + (va * rf(m, h + 0x34))) + (vc * rf(m, h + 0x54))) + rf(m, h + 0x64);
    let t62 = (((va * rf(m, h + 0x38)) + (vb * rf(m, h + 0x48))) + (vc * rf(m, h + 0x58))) + rf(m, h + 0x68);
    let t3x = ((vc * rf(m, h + 0x5c)) + ((va * rf(m, h + 0x3c)) + (vb * rf(m, h + 0x4c)))) + rf(m, h + 0x6c);
    let t63 = x(1.0f32) / p7;
    let fx = x((t61 * t63).to_f32());
    let fy = x((t62 * t63).to_f32());
    let fz = x((t3x * t63).to_f32());
    if k1 == 2 {
        let t67 = (((fx * rf(m, g + 0x178)) + (fy * rf(m, g + 0x188))) + (fz * rf(m, g + 0x198))) + rf(m, g + 0x1a8);
        let t68 = (((fx * rf(m, g + 0x17c)) + (fy * rf(m, g + 0x18c))) + (fz * rf(m, g + 0x19c))) + rf(m, g + 0x1ac);
        let t69 = (((fy * rf(m, g + 0x190)) + (fx * rf(m, g + 0x180))) + (fz * rf(m, g + 0x1a0))) + rf(m, g + 0x1b0);
        W!(fp - 0x9c, ((rf(m, p1 + 0x17c) * t68) + (rf(m, p1 + 0x178) * t67)) + (rf(m, p1 + 0x180) * t69));
        W!(fp - 0x98, ((rf(m, p1 + 0x188) * t67) + (rf(m, p1 + 0x18c) * t68)) + (rf(m, p1 + 0x190) * t69));
        W!(fp - 0x94, (t69 * rf(m, p1 + 0x1a0)) + ((t68 * rf(m, p1 + 0x19c)) + (t67 * rf(m, p1 + 0x198))));
    } else if k1 == 0xc {
        if c == 0 {
            W!(fp - 0x9c, (((fx * rf(m, g + 0x178)) + (fy * rf(m, g + 0x188))) + (fz * rf(m, g + 0x198))) + rf(m, g + 0x1a8));
            W!(fp - 0x98, (((fy * rf(m, g + 0x18c)) + (fx * rf(m, g + 0x17c))) + (fz * rf(m, g + 0x19c))) + rf(m, g + 0x1ac));
            W!(fp - 0x94, (((fy * rf(m, g + 0x190)) + (fx * rf(m, g + 0x180))) + (fz * rf(m, g + 0x1a0))) + rf(m, g + 0x1b0));
        } else {
            let t64 = (((fx * rf(m, g + 0x178)) + (fy * rf(m, g + 0x188))) + (fz * rf(m, g + 0x198))) + rf(m, g + 0x1a8);
            let t65 = (((fx * rf(m, g + 0x17c)) + (fy * rf(m, g + 0x18c))) + (fz * rf(m, g + 0x19c))) + rf(m, g + 0x1ac);
            let t66 = ((fz * rf(m, g + 0x1a0)) + ((fy * rf(m, g + 0x190)) + (fx * rf(m, g + 0x180)))) + rf(m, g + 0x1b0);
            W!(fp - 0x9c, t64);
            W!(fp - 0x98, t65);
            W!(fp - 0x94, t66);
            W!(fp - 0x9c, ((rf(m, c + 0x178) * t64) + (rf(m, c + 0x17c) * t65)) + (rf(m, c + 0x180) * t66));
            W!(fp - 0x98, ((rf(m, c + 0x188) * t64) + (rf(m, c + 0x18c) * t65)) + (rf(m, c + 0x190) * t66));
            W!(fp - 0x94, (t66 * rf(m, c + 0x1a0)) + ((t65 * rf(m, c + 0x19c)) + (t64 * rf(m, c + 0x198))));
        }
    }
    let (o9c, o98, o94) = (m.u32((fp - 0x9c) as u32), m.u32((fp - 0x98) as u32), m.u32((fp - 0x94) as u32));
    m.set_u32((p4 + 0x14) as u32, o9c);
    m.set_u32((p4 + 0x18) as u32, o98);
    m.set_u32((p4 + 0x1c) as u32, o94);
    1
}

/// The sum of the response over every contact point of a multiple collision
/// [soft 0x084a8a34 si_resolveMultipleCollision]: each point of the list at `info + 0x38`
/// (count at +0, the points at the array behind +8, the contact models at the array behind
/// +0x80) is copied to `info + 0x20..0x28`, `resolveCollision` is run for it with the speed
/// factors of its model's dynamics record (the second model's when it has none), and the
/// result at `info + 0x14..0x1c` is divided by the count; the divided impulses and their
/// moments about the point (through the joint matrix at `jv + 0x220..0x240`) are summed.
/// With more than three points one more run is made at the mean point, and its moment
/// replaces the summed one. The sums end in `info + 0x14..0x1c` and `info + 0x2c..0x34`;
/// `info + 0x20..0x28` ends at 0.
pub fn si_resolve_multiple_collision(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32, jv: u32, info: u32, p5: f32) {
    let ri = |m: &Mem, a: u32| m.i32(a);
    let rf = |m: &Mem, a: u32| x(m.f32(a));
    let (mut l_c, mut l_8, mut l_4) = (0f32, 0f32, 0f32); // the sums of the impulse
    let (mut l_18, mut l_14, mut l_10) = (0f32, 0f32, 0f32); // the sums of the moment
    let (mut s_x, mut s_y, mut s_z) = (0f32, 0f32, 0f32); // the sum of the points
    let list = ri(m, info + 0x38) as u32;
    let mut list_now = list;
    let mut i: i32 = 0;
    let mut off: u32 = 0;
    // the factor pair of a contact model record, the second model's when it has none
    let factors = |m: &Mem, rec: u32| -> (f32, f32) {
        let r = if rec == 0 { ri(m, p2 + 0x2f4) as u32 } else { rec };
        (m.f32(r + 0xc), m.f32(r + 0x14))
    };
    if 0 < ri(m, list) {
        loop {
            let pts = ri(m, list_now + 8) as u32;
            for k in [0u32, 4, 8] {
                let w = m.u32(pts + off + k);
                m.set_u32(info + 0x20 + k, w);
            }
            s_x = (x(s_x) + rf(m, info + 0x20)).to_f32();
            s_y = (x(s_y) + rf(m, info + 0x24)).to_f32();
            s_z = (x(s_z) + rf(m, info + 0x28)).to_f32();
            m.set_u32(info + 0x1c, 0);
            m.set_u32(info + 0x18, 0);
            m.set_u32(info + 0x14, 0);
            let cm = ri(m, list_now + 0x80) as u32;
            let cm = ri(m, cm.wrapping_add((i as u32).wrapping_mul(4))) as u32;
            let rec = ri(m, cm + 0x2f4) as u32;
            let (f1, f2) = factors(m, rec);
            resolve_collision(m, o, p1, p2, jv, info, f1, f2, p5);
            list_now = ri(m, info + 0x38) as u32;
            let n = x(ri(m, list_now));
            let r = F80::ONE / n;
            let xx = (r * rf(m, info + 0x14)).to_f32();
            let yy = (r * rf(m, info + 0x18)).to_f32();
            let zz = (r * rf(m, info + 0x1c)).to_f32();
            m.set_f32(info + 0x14, xx);
            m.set_f32(info + 0x18, yy);
            m.set_f32(info + 0x1c, zz);
            l_c = (x(l_c) + x(xx)).to_f32();
            l_8 = (x(l_8) + x(yy)).to_f32();
            l_4 = (x(l_4) + x(zz)).to_f32();
            let (xr, yr, zr) = (x(xx), x(yy), x(zz));
            let t1 = ((rf(m, jv + 0x220) * xr) + (rf(m, jv + 0x224) * yr)) + (rf(m, jv + 0x228) * zr);
            let t2 = ((rf(m, jv + 0x22c) * xr) + (rf(m, jv + 0x230) * yr)) + (rf(m, jv + 0x234) * zr);
            let t3 = (zr * rf(m, jv + 0x240)) + ((yr * rf(m, jv + 0x23c)) + (xr * rf(m, jv + 0x238)));
            let (q, rr, pp) = (rf(m, info + 0x24), rf(m, info + 0x28), rf(m, info + 0x20));
            let t4 = (q * t3) - (rr * t2);
            let t5 = (t2 * pp) - (q * t1);
            let t6 = (t1 * rr) - (t3 * pp);
            l_18 = (t4 + x(l_18)).to_f32();
            l_10 = (t5 + x(l_10)).to_f32();
            l_14 = (t6 + x(l_14)).to_f32();
            i += 1;
            off = off.wrapping_add(0xc);
            if !(i < ri(m, list_now)) {
                break;
            }
        }
    }
    m.set_u32(info + 0x28, 0);
    m.set_u32(info + 0x24, 0);
    m.set_u32(info + 0x20, 0);
    m.set_f32(info + 0x14, l_c);
    m.set_f32(info + 0x18, l_8);
    m.set_f32(info + 0x1c, l_4);
    m.set_f32(info + 0x2c, l_18);
    m.set_f32(info + 0x30, l_14);
    m.set_f32(info + 0x34, l_10);
    let list = ri(m, info + 0x38) as u32;
    let n = ri(m, list);
    if n > 3 {
        let r = F80::ONE / x(n);
        let ax = (r * x(s_x)).to_f32();
        let ay = (r * x(s_y)).to_f32();
        let az = (r * x(s_z)).to_f32();
        m.set_f32(info + 0x20, ax);
        m.set_f32(info + 0x24, ay);
        m.set_f32(info + 0x28, az);
        let first = ri(m, ri(m, list + 0x80) as u32) as u32;
        let rec = ri(m, first + 0x2f4) as u32;
        let (f1, f2) = factors(m, rec);
        resolve_collision(m, o, p1, p2, jv, info, f1, f2, p5);
        let (xr, yr, zr) = (rf(m, info + 0x14), rf(m, info + 0x18), rf(m, info + 0x1c));
        let t7 = ((rf(m, jv + 0x224) * yr) + (rf(m, jv + 0x220) * xr)) + (rf(m, jv + 0x228) * zr);
        let t8 = ((rf(m, jv + 0x22c) * xr) + (rf(m, jv + 0x230) * yr)) + (rf(m, jv + 0x234) * zr);
        let t9 = (zr * rf(m, jv + 0x240)) + ((yr * rf(m, jv + 0x23c)) + (xr * rf(m, jv + 0x238)));
        let (q, rr, pp) = (rf(m, info + 0x24), rf(m, info + 0x28), rf(m, info + 0x20));
        m.set_f32(info + 0x2c, ((q * t9) - (rr * t8)).to_f32());
        m.set_u32(info + 0x28, 0);
        m.set_u32(info + 0x24, 0);
        m.set_u32(info + 0x20, 0);
        m.set_f32(info + 0x34, ((t8 * pp) - (t7 * q)).to_f32());
        m.set_f32(info + 0x30, ((rr * t7) - (t9 * pp)).to_f32());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_two_by_two_system() {
        // 2x + y = 5, x + 3y = 10 as the augmented matrix [2 1 5; 1 3 10].
        let mut m = Mem::new();
        let a = m.malloc(64);
        for (i, v) in [2.0f32, 1.0, 5.0, 1.0, 3.0, 10.0].iter().enumerate() {
            m.set_f32(a + 4 * i as u32, *v);
        }
        assert_ne!(resolve_linear_system_rg(&mut m, a, 2, 3, 2), 0);
        assert!((m.f32(a + 8) - 1.0).abs() < 1e-5);
        assert!((m.f32(a + 20) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn singular_system_reports_zero() {
        let mut m = Mem::new();
        let a = m.malloc(64);
        for (i, v) in [0.0f32, 0.0, 1.0, 0.0, 0.0, 2.0].iter().enumerate() {
            m.set_f32(a + 4 * i as u32, *v);
        }
        assert_eq!(resolve_linear_system_rg(&mut m, a, 2, 3, 2), 0);
    }
}
