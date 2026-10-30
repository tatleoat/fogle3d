// SPDX-License-Identifier: MIT
//! `range.c`: the metaballs' bounding ranges (finding 0012, section 28).

use crate::volume::*;
use si_mem::Mem;
use si_x87::F80;

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f64(m.f64(a))
}

/// [soft 0x0857a360 VC_getmaxmin]: `VC_getmaxmin(out)`: the box (six doubles: low corner, then high corner) round
/// all metaballs in use, from their translation and half extents. Compares the unrounded sums, stores rounded.
pub fn get_max_min(m: &mut Mem, out: u32) {
    for k in 0..3 {
        m.set_f64(out + 8 * k, 1e99);
        m.set_f64(out + 24 + 8 * k, -1e99);
    }
    let mut i = 0;
    while i < m.i32(VCG_MAX_MBS) {
        let b = mb_at(m, i);
        if m.i16(b + mb::FREE) == 0 {
            for k in 0..3u32 {
                let t = f(m, b + 0x12c + 8 * k);
                let e = f(m, b + mb::EXTENT + 8 * k);
                let (lo, hi) = (t - e, t + e);
                if f(m, out + 8 * k).gt(lo) {
                    m.set_f64(out + 8 * k, lo.to_f64());
                }
                if f(m, out + 24 + 8 * k).lt(hi) {
                    m.set_f64(out + 24 + 8 * k, hi.to_f64());
                }
            }
        }
        i += 1;
    }
}

/// [soft 0x0857a4a4 VC_influ_test]: `VC_influ_test(list, point, rx, ry)`: 1 when the box of some listed metaball meets
/// the box round `point` (half sizes `rx`, `ry` and 0 in z), else 0.
pub fn influ_test(m: &mut Mem, list: u32, pt: u32, rx: f64, ry: f64) -> i32 {
    let (p0, p1, p2) = (f(m, pt), f(m, pt + 8), m.f64(pt + 16));
    let lo = [(p0 - F80::from_f64(rx)).to_f64(), (p1 - F80::from_f64(ry)).to_f64(), p2];
    let hi = [(F80::from_f64(rx) + p0).to_f64(), (F80::from_f64(ry) + p1).to_f64(), p2];
    let mut i = 0;
    while i < m.i32(list) {
        let b = mb_at(m, m.i32(m.u32(list + 8) + 4 * i as u32));
        let mut hit = true;
        for k in 0..3u32 {
            let t = f(m, b + 0x12c + 8 * k);
            let e = f(m, b + mb::EXTENT + 8 * k);
            if !((t - e).lt(F80::from_f64(hi[k as usize])) && (t + e).gt(F80::from_f64(lo[k as usize]))) {
                hit = false;
                break;
            }
        }
        if hit {
            return 1;
        }
        i += 1;
    }
    0
}

/// [soft 0x0857a5c4 VC_extract_Zslab]: `VC_extract_Zslab(list, z, width)`: the metaballs in use whose z reaches the slab,
/// their indices into the list, and the count.
pub fn extract_zslab(m: &mut Mem, list: u32, z: f64, width: f64) {
    let mut n = 0u32;
    let mut i = 0;
    while i < m.i32(VCG_MAX_MBS) {
        let b = mb_at(m, i);
        if m.i16(b + mb::FREE) == 0 {
            let mut d1 = F80::from_f64(z) - f(m, b + 0x13c);
            let d2 = f(m, b + mb::EXTENT + 16) + F80::from_f64(width);
            if d1.lt(F80::ZERO) {
                d1 = -d1;
            }
            if d1.lt(d2) {
                let p = m.u32(list + 8);
                m.set_i32(p + 4 * n, i);
                n += 1;
            }
        }
        i += 1;
    }
    m.set_u32(list, n);
}

/// [soft 0x0857a644 VC_getBoundingVolume]: `VC_getBoundingVolume(index)`: sets the metaball's half extents from the box round
/// its transformed cube corners, grown by a tenth.
pub fn get_bounding_volume(m: &mut Mem, index: i32) {
    let mut lo = [1e99f64; 3];
    let mut hi = [-1e99f64; 3];
    let b = mb_at(m, index);
    let mut mat = [[0f64; 4]; 4];
    for (r, row) in mat.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = m.f64(b + mb::MAT_IN + 32 * r as u32 + 8 * c as u32);
        }
    }
    for c in 0..8u32 {
        let corner = [m.f64(CORNERS + 24 * c), m.f64(CORNERS + 24 * c + 8), m.f64(CORNERS + 24 * c + 16)];
        // VC_Vec3XformSimple (0x0857c2e0) is si-math's port.
        let v = si_math::vector::vc_vec3_xform_simple(&corner, &mat);
        for k in 0..3 {
            if lo[k] > v[k] {
                lo[k] = v[k];
            }
            if hi[k] < v[k] {
                hi[k] = v[k];
            }
        }
    }
    let b = mb_at(m, index);
    for k in 0..3usize {
        let e = ((F80::from_f64(hi[k]) - F80::from_f64(lo[k])) * F80::from_f64(0.5)) * F80::from_f64(1.1);
        m.set_f64(b + mb::EXTENT + 8 * k as u32, e.to_f64());
    }
}

/// [soft 0x0857a788 VC_check_upbv]: `VC_check_upbv(a, b, c)`: 1 when the integer box `a` (x0, y0, ., x1, y1) meets the square round `b`
/// of half side `c`.
pub fn check_upbv(m: &Mem, a: u32, b: u32, c: i32) -> i32 {
    let (a0, a1, a3, a4) = (m.i32(a), m.i32(a + 4), m.i32(a + 12), m.i32(a + 16));
    let (b0, b1) = (m.i32(b), m.i32(b + 4));
    (a0 < b0.wrapping_add(c) && a1 < c.wrapping_add(b1) && b0.wrapping_sub(c) < a3 && b1.wrapping_sub(c) < a4) as i32
}
