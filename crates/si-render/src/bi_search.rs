// SPDX-License-Identifier: MIT
//! `bi_search.c`: bisection for the surface point on a segment (finding 0012, section 28).

use crate::vceval::eval_dens;
use si_mem::Mem;
use si_x87::F80;

/// [soft 0x0857c8e0 VC_bi_search]: `VC_bi_search(list, a, b)`: `a` is a point with density under 1, `b` one above; bisects at most ten
/// times towards density 1 (stopping when it is within 0.01), leaves the point in `a` (or `b` if the last density was 0).
pub fn bi_search(m: &mut Mem, list: u32, a: u32, b: u32) {
    let one = F80::ONE;
    let half = F80::from_f64(0.5);
    let mut lo = [m.f64(a), m.f64(a + 8), m.f64(a + 16)];
    let mut hi = [m.f64(b), m.f64(b + 8), m.f64(b + 16)];
    let mut i = 0;
    let mut d;
    loop {
        for k in 0..3usize {
            m.set_f64(a + 8 * k as u32, ((F80::from_f64(lo[k]) + F80::from_f64(hi[k])) * half).to_f64());
        }
        d = eval_dens(m, list, a);
        let r;
        if d.gt(one) {
            for k in 0..3usize {
                hi[k] = m.f64(a + 8 * k as u32);
            }
            r = d - one;
        } else if d.lt(one) {
            for k in 0..3usize {
                lo[k] = m.f64(a + 8 * k as u32);
            }
            r = one - d;
        } else {
            r = F80::ZERO;
        }
        i += 1;
        if !(r.gt(F80::from_f64(0.01)) && i <= 9) {
            break;
        }
    }
    if F80::ZERO.eq(d) {
        for k in 0..3u32 {
            let v = m.f64(b + 8 * k);
            m.set_f64(a + 8 * k, v);
        }
    }
}
