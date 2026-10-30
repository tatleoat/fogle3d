// SPDX-License-Identifier: MIT
//! `nrb_draw.c` sampling routines on the memory image (finding 0025 section 27): `SI_nurbsCrvSamples` as the heap-making form
//! and `si_nurbsSrfSampleTrims`, which turns each trim composite's curves into one polyline of (x, y) pairs.

use crate::image::{crv_value, Halt};
use si_mem::Mem;
use si_x87::{Round, F80};

/// [soft 0x084d7fec SI_nurbsCrvSamples] on the image: `(curve, mode, step, &count, &array)`. Nothing is done unless `mode` is 0.
/// `count = floor((end - start) / step) + 1` (the quotient floored in the register, stored as a double, converted truncating);
/// `*array = SI_calloc(count, 0x18)`; then for each of the `count` samples the parameter (kept in the register, lowered to
/// the range end when above it, rounded to a double for the call) is evaluated by `SI_nurbsCrvValue` and its three doubles
/// stored; the register parameter then grows by `step`.
pub fn crv_samples_img(m: &mut Mem, c: u32, mode: i32, step: f64, count_out: u32, array_out: u32) -> Result<(), Halt> {
    if mode != 0 {
        return Ok(());
    }
    let (start, end) = (m.f64(c + 0x10), m.f64(c + 0x18));
    let q = (F80::from_f64(end) - F80::from_f64(start)) / F80::from_f64(step);
    let n = F80::from_f64(q.round_int(Round::Down).to_f64()).to_i32(Round::Zero);
    let count = n.wrapping_add(1);
    m.set_i32(count_out, count);
    if count > 1 << 20 {
        return Err(Halt::Refused("SI_nurbsCrvSamples"));
    }
    let arr = m.si_calloc(count as u32, 0x18);
    m.set_u32(array_out, arr);
    let mut t = F80::from_f64(start);
    for i in 0..count.max(0) {
        let re = F80::from_f64(end);
        if re.lt(t) {
            t = re;
        }
        let v = crv_value(m, c, t.to_f64(), 0)?;
        for (k, x) in v.iter().enumerate() {
            m.set_f64(arr + 0x18 * i as u32 + 8 * k as u32, *x);
        }
        t = t + F80::from_f64(step);
    }
    Ok(())
}

/// The step `si_nurbsSrfSampleTrims` samples its curves with (the double at 0x08661848).
pub const SAMPLE_STEP: f64 = 0.002;

/// [soft 0x084e5228 si_nurbsSrfSampleTrims]: `(srf)`: for each of the `srf + 0x60` trim composites (array at `srf + 0x64`): its
/// total at +0x10 is 0, then each of its `+4` curves (pointers at the array `+8`) is sampled by [`crv_samples_img`] with
/// mode 0 and step 0.002, the total growing by `count - 1`; the composite's table at +0x14 (`SI_calloc(total, 0x18)`) receives
/// x and y of samples 1.. of each curve in turn (24 bytes apart), and each sample array is freed once copied.
pub fn sample_trims(m: &mut Mem, s: u32) -> Result<(), Halt> {
    let sp0 = m.sp();
    let loc = m.local(0x320 - 0x190 + 0x190);
    let (cnt, ptr) = (loc, loc + 0x190);
    let mut t = 0;
    while t < m.i32(s + 0x60) {
        let comp = m.u32(m.u32(s + 0x64).wrapping_add(4 * t as u32));
        m.set_u32(comp + 0x10, 0);
        let nc = m.i32(comp + 4);
        if nc > 100 {
            return Err(Halt::Refused("si_nurbsSrfSampleTrims"));
        }
        for i in 0..nc.max(0) as u32 {
            let crv = m.u32(m.u32(comp + 8).wrapping_add(4 * i));
            crv_samples_img(m, crv, 0, SAMPLE_STEP, cnt + 4 * i, ptr + 4 * i)?;
            let tot = m.i32(comp + 0x10).wrapping_sub(1).wrapping_add(m.i32(cnt + 4 * i));
            m.set_i32(comp + 0x10, tot);
        }
        let tab = m.si_calloc(m.i32(comp + 0x10) as u32, 0x18);
        m.set_u32(comp + 0x14, tab);
        let mut k = 0u32;
        for i in 0..nc.max(0) as u32 {
            let src = m.u32(ptr + 4 * i);
            for j in 1..m.i32(cnt + 4 * i).max(1) as u32 {
                if src == 0 {
                    return Err(Halt::Refused("si_nurbsSrfSampleTrims"));
                }
                let (x, y) = (m.f64(src + 0x18 * j), m.f64(src + 0x18 * j + 8));
                m.set_f64(tab + k, x);
                m.set_f64(tab + k + 8, y);
                k += 0x18;
            }
            if src != 0 {
                m.free(src);
                m.set_u32(ptr + 4 * i, 0);
            }
        }
        t += 1;
    }
    m.set_sp(sp0);
    Ok(())
}
