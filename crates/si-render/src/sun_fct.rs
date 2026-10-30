// SPDX-License-Identifier: MIT
//! Two helpers of the sun light's position: the original's `sun_fct.c` (finding 0012,
//! section 20), `latlong2deg` and `eqTime`, on the memory image. `SI_lightSunGetPosition`,
//! which calls them, is not ported.

use si_mem::Mem;
use si_x87::{x, F80};

/// `ctab` (0x086a2ec0): fourteen pairs of a day of the year (i32) and the equation of time
/// at that day (f32); in the binary's data ([`crate::data::TABLES`]).
pub const CTAB: u32 = 0x086a_2ec0;
/// 0x086559c8 (double): minutes in a degree.
const SIXTY: f64 = 60.0;
/// 0x086559b0, 0x086559b8, 0x086559c0 (double): the length of the second stretch of the
/// table, of the thirteenth, and of every other.
const DAYS_1: f64 = 29.0;
const DAYS_12: f64 = 6.0;
const DAYS: f64 = 30.0;

/// Degrees and minutes as degrees [soft 0x082a6bdc latlong2deg]: `latlong2deg(value)`:
/// r = value % 60 and q = (value − r) / 60 (C's truncating division and remainder); q +
/// r / 60.0 in the register, the division by the double 60 and the sum unrounded, then
/// stored as a float and loaded again: the answer is that float. Class: model.
pub fn latlong2deg(value: i32) -> F80 {
    let r = value % 60;
    let q = (value - r) / 60;
    x((x(q) + x(r) / x(SIXTY)).to_f32())
}

/// The equation of time on a day [soft 0x082a6b50 eqTime]: `eqTime(&tm)`, the day of the
/// year read at +0x1c.
///
/// k is the first index from 1 whose day is above the day of the year (1 when the day is
/// below the second entry's), or 14 when none of the first 13 is: then 0. Else j = k − 1,
/// t = float((day − day_j) / L) with L the double 29 for j = 1, 6 for j = 12, else 30;
/// the answer, in the register, is t × value_k + (1 − t) × value_j (each value a float of
/// the table). Class: model.
pub fn eq_time(m: &Mem, tm: u32) -> F80 {
    let yday = m.i32(tm.wrapping_add(0x1c));
    let day = |k: u32| m.i32(CTAB + 8 * k);
    let value = |k: u32| m.f32(CTAB + 4 + 8 * k);
    let mut k = 1u32;
    if yday >= day(1) {
        loop {
            k += 1;
            if k > 13 || yday < day(k) {
                break;
            }
        }
    }
    if k == 14 {
        return F80::ZERO;
    }
    let j = k - 1;
    let length = match j {
        1 => DAYS_1,
        12 => DAYS_12,
        _ => DAYS,
    };
    let t = x((x(yday.wrapping_sub(day(j))) / x(length)).to_f32());
    t * value(k) + (F80::ONE - t) * value(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_become_fractions_of_a_degree() {
        assert_eq!(latlong2deg(4530).to_f32(), 75.5);
        assert_eq!(latlong2deg(-4530).to_f32(), -75.5);
        assert_eq!(latlong2deg(59).to_f32(), (59.0f64 / 60.0) as f32);
    }

    #[test]
    fn the_table_is_interpolated() {
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        crate::data::init(&mut m);
        let tm = m.calloc(1, 0x2c);
        // day 0 is the first entry's value; day 15 half way to the second
        m.set_i32(tm + 0x1c, 0);
        assert_eq!(eq_time(&m, tm).to_f32(), f32::from_bits(0x3f51eb85));
        m.set_i32(tm + 0x1c, 15);
        let want = (x(0.5f32) * f32::from_bits(0x40566666) + (F80::ONE - x(0.5f32)) * f32::from_bits(0x3f51eb85)).to_f32();
        assert_eq!(eq_time(&m, tm).to_f32(), want);
        // past the thirteenth entry's day: 0
        m.set_i32(tm + 0x1c, 400);
        assert_eq!(eq_time(&m, tm).to_bits(), F80::ZERO.to_bits());
    }
}
