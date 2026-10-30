// SPDX-License-Identifier: MIT
//! The contour fit's driver of `fitCurve.c` on the one memory image (finding 0040, section 2):
//! `SI_bezierFitContour`, which turns a traced picture contour (points and one flag word a
//! point) into a Bézier spline, with the helpers of [`crate::contour_image`] and
//! [`crate::fit_image`] doing the fitting.
//!
//! The contour record (`my_contour`, read from code): the number of points at +0 and, at +4,
//! the address of that many triples of `float`.

use si_mem::{Mem, Ptr32};
use si_types::geom::Spline;
use si_x87::{x, F80};

use crate::contour_image::{generate_curve, new_point, simplify_contour};
use crate::fit_image::{fit_bez_curve, PNT};
use crate::spline_image::{spline_allocate, spline_delete_point, spline_update_matrix, Outside, FIT_ERROR, JOIN, LOOK_AHEAD, SI_C_BEZ};

/// The distance of two points of floats, as the original sums it: the differences exact in
/// the register, `(dx² + dy²) + dz²`, the square root of that, nothing stored.
fn dist(a: [f32; 3], b: [f32; 3]) -> F80 {
    let dx = x(a[0]) - b[0];
    let dy = x(a[1]) - b[1];
    let sxy = dx * dx + dy * dy;
    let dz = x(a[2]) - b[2];
    (sxy + dz * dz).sqrt()
}

fn v3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8))]
}

/// The point at `ret` moved towards the line, as the original does at the two handles of a
/// segment: `newPoint(pos, handle, d)` stored over the handle.
fn pull(m: &mut Mem, handle: u32, pos: u32, d: f64) {
    let (a, b) = (v3(m, pos), v3(m, handle));
    new_point(m, handle, a, b, d);
}

/// Fits a Bézier spline to a contour [soft 0x084379dc SI_bezierFitContour] and returns it
/// (null when no spline could be allocated). `flags` has one 16-bit word a point (non-zero:
/// a corner).
///
/// 1. `join` is cleared; a new spline of type 1 with its matrix becomes `SI_cBez`; the
///    points go (as doubles) into `SI_malloc((n × 3 + 6) × 8)`, `n` being the contour's
///    count; the first and last point are kept as doubles.
/// 2. With more than `2 × lookAhead` intervals (`n − 1`): the first corner flag among the
///    first `n − 2` points (0 when none) is the start; the points and flags are copied,
///    starting there and wrapping modulo `n − 1`, into two new blocks; for each later
///    corner `i` (index `1 ..n − 1`), [`simplify_contour`] and [`generate_curve`] fit the
///    run from the last corner to `i`, then the last run; the blocks are freed. With fewer,
///    [`fit_bez_curve`] fits all `n − 1` points at once (`fitError`) and `join` is set.
///    (A look-ahead small enough to make `n − 1` zero would divide by zero in the
///    original; the Rust does the same.)
/// 3. For each segment but the last, with `d1` the distance of its start from its outgoing
///    handle, `d2` that of its end from its incoming handle and `d3` the chord (all in the
///    register): when `d3 < d1 + d2 < 4 × d3` both handles are moved with `newPoint` by the
///    ratio `d3 / (d1 + d2)` rounded to a double.
/// 4. When the first and last points are less than 10 apart the spline is closed: the first
///    key's incoming handle is the last key's, the last key is deleted
///    ([`spline_delete_point`]), the two end handles are adjusted by the same rule, and the
///    closed word (+8) is set to 1 (also when the adjustment did not apply).
/// 5. The array of points is freed.
pub fn bezier_fit_contour(m: &mut Mem, o: &mut dyn Outside, contour: u32, flags: u32) -> Ptr32<Spline> {
    m[JOIN] = 0;
    let s = spline_allocate(m);
    m[SI_C_BEZ] = s;
    if s.is_null() {
        return s;
    }
    m[s].kind = 1;
    spline_update_matrix(m, s);
    let n = m.i32(contour);
    let n1 = n.wrapping_sub(1);
    let buf = m.si_malloc(n.wrapping_mul(3).wrapping_add(6).wrapping_mul(8) as u32);
    if n1 >= 0 {
        for i in 0..n as u32 {
            let src = m.u32(contour + 4).wrapping_add(12 * i);
            for k in 0..3 {
                let v = m.f32(src + 4 * k) as f64;
                m.set_f64(buf.wrapping_add(i * 0x18 + 8 * k), v);
            }
        }
    }
    let pts = m.u32(contour + 4);
    let first: [f64; 3] = core::array::from_fn(|k| m.f32(pts + 4 * k as u32) as f64);
    let lastp = pts.wrapping_add((n1 as u32).wrapping_mul(12));
    let last: [f64; 3] = core::array::from_fn(|k| m.f32(lastp.wrapping_add(4 * k as u32)) as f64);

    if m[LOOK_AHEAD].wrapping_mul(2) < n1 {
        let mut start: i32 = 0;
        if 0 < n1 && m.u16(flags) == 0 {
            let mut i = 0;
            loop {
                i += 1;
                if i >= n1 {
                    break;
                }
                if m.u16(flags.wrapping_add((i as u32) << 1)) != 0 {
                    start = i;
                    break;
                }
            }
        }
        let count = n1.wrapping_add(1);
        let copy = m.si_malloc((count as u32).wrapping_mul(PNT));
        let fl = m.si_malloc((count as u32).wrapping_mul(2));
        let end = start.wrapping_add(n1).wrapping_add(1);
        let mut j = start;
        let mut k = 0u32;
        while j < end {
            // the original divides by n1 here (zero: a fault)
            let idx = (j % n1) as u32;
            m.copy(copy.wrapping_add(k * PNT), buf.wrapping_add(idx.wrapping_mul(PNT)), PNT);
            let f = m.u16(flags.wrapping_add(idx << 1));
            m.set_u16(fl.wrapping_add(k << 1), f);
            j += 1;
            k += 1;
        }
        let frame = m.local(12);
        let (out_pts, out_flags, out_n) = (frame, frame + 4, frame + 8);
        let mut s0: i32 = 0;
        let mut i: i32 = 1;
        while i < n1 {
            if m.u16(fl.wrapping_add((i as u32) << 1)) != 0 {
                let run = copy.wrapping_add((s0 as u32).wrapping_mul(PNT));
                simplify_contour(m, run, i - s0 + 1, out_pts, out_flags, out_n);
                let (p, f, c) = (m.u32(out_pts), m.u32(out_flags), m.i32(out_n));
                generate_curve(m, p, f, c);
                let p = m.u32(out_pts);
                if p != 0 {
                    m.free(p);
                    m.set_u32(out_pts, 0);
                }
                let f = m.u32(out_flags);
                if f != 0 {
                    m.free(f);
                    m.set_u32(out_flags, 0);
                }
                s0 = i;
            }
            i += 1;
        }
        let rest = (n1 - s0).wrapping_add(1);
        if rest > 1 {
            let run = copy.wrapping_add((s0 as u32).wrapping_mul(PNT));
            simplify_contour(m, run, rest, out_pts, out_flags, out_n);
            let (p, f, c) = (m.u32(out_pts), m.u32(out_flags), m.i32(out_n));
            generate_curve(m, p, f, c);
            let p = m.u32(out_pts);
            if p != 0 {
                m.free(p);
                m.set_u32(out_pts, 0);
            }
            let f = m.u32(out_flags);
            if f != 0 {
                m.free(f);
                m.set_u32(out_flags, 0);
            }
        }
        m.release(12);
        if copy != 0 {
            m.free(copy);
        }
        if fl != 0 {
            m.free(fl);
        }
    } else {
        let error = m.f64(FIT_ERROR.addr());
        fit_bez_curve(m, buf, n1, error);
        m[JOIN] = 1;
    }

    // the handles of the segments but the last are pulled towards the chord
    let mut i: i32 = 0;
    while i < (m[s].nb_keys as u16 as i32) - 2 {
        let k0 = m[s].keys.addr().wrapping_add((i as u32).wrapping_mul(0x30));
        let k1 = k0.wrapping_add(0x30);
        let d1 = dist(v3(m, k0 + 0xc), v3(m, k0 + 0x18));
        let d2 = dist(v3(m, k1 + 0xc), v3(m, k1));
        let d3 = dist(v3(m, k0 + 0xc), v3(m, k1 + 0xc));
        let sum = d1 + d2;
        if sum.gt(d3) && (d3 * x(4.0f64)).gt(sum) {
            let ratio = (d3 / sum).to_f64();
            pull(m, k0 + 0x18, k0 + 0xc, ratio);
            pull(m, k1, k1 + 0xc, ratio);
        }
        i += 1;
    }

    let dx = x(first[0]) - last[0];
    let dy = x(first[1]) - last[1];
    let sxy = dx * dx + dy * dy;
    let dz = x(first[2]) - last[2];
    if (sxy + dz * dz).sqrt().lt(x(10.0f64)) {
        let keys = m[s].keys.addr();
        let nk = m[s].nb_keys as u16 as u32;
        let lastk = keys.wrapping_add(nk.wrapping_mul(0x30)).wrapping_sub(0x30);
        for w in 0..3 {
            let v = m.u32(lastk + 4 * w);
            m.set_u32(keys + 4 * w, v);
        }
        let nk = (m[s].nb_keys as u16 as i32).wrapping_sub(1);
        spline_delete_point(m, o, s, nk);
        let keys = m[s].keys.addr();
        let nk = m[s].nb_keys as u16 as u32;
        let e = keys.wrapping_add(nk.wrapping_mul(0x30));
        // the last key: position at e-0x24, outgoing handle at e-0x18
        let a1 = dist(v3(m, e.wrapping_sub(0x24)), v3(m, e.wrapping_sub(0x18)));
        let a2 = dist(v3(m, keys + 0xc), v3(m, keys));
        let c = dist(v3(m, e.wrapping_sub(0x24)), v3(m, keys + 0xc));
        let sum = a1 + a2;
        if sum.gt(c) {
            let ratio = (c / sum).to_f64();
            pull(m, e.wrapping_sub(0x18), e.wrapping_sub(0x24), ratio);
            pull(m, keys, keys + 0xc, ratio);
        }
        m[s].closed = 1;
    }
    if buf != 0 {
        m.free(buf);
    }
    s
}
