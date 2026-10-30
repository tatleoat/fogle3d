// SPDX-License-Identifier: MIT
//! `SI_lightSunGetPosition` of `sun_fct.c` (finding 0023, section 36): where the sun light's
//! position puts the sun, as a vector of length 20. The two helpers (`latlong2deg`, `eqTime`)
//! are in [`crate::sun_fct`]. The trigonometric library calls go through `si_x87::trig`
//! (replayed from the original's answers in a check); `localtime` is the C library's, asked
//! through the outside world and answered with a `struct tm` in the image.

use crate::sun_fct::{eq_time, latlong2deg};
use si_mem::Mem;
use si_x87::{trig, x, Round, F80};

si_mem::outside_calls! {
    /// The C library call `SI_lightSunGetPosition` makes.
    pub trait SunCalls {
        /// `localtime (0x080c3a98)`: (address of a `time_t`): the broken-down time, or 0.
        fn localtime(timep: u32) -> u32 = "localtime", 0x080c3a98;
    }
}

/// 0x086559d0 .. 0x086559f8 (double), 0x08655a08, 0x08655a10 (double); 0x086559e8, 0x08655a00,
/// 0x08655a18, 0x08655a1c, 0x08655a20, 0x08655a24 (float).
const SIXTY: f64 = 60.0;
const TWELVE: f64 = 12.0;
const FIFTEEN_D: f64 = 15.0;
const FIFTEEN_F: f32 = 15.0;
const HALF_STEP: f64 = 7.5;
const RADIANS: f64 = 0.0174532925;
const EIGHTY: f32 = 80.0;
const YEAR: f64 = 365.25;
const TWO_PI: f64 = 6.283185307179586;
const TILT: f32 = 0.4084070324897766;
const MINUS_ONE: f32 = -1.0;
const RADIUS: f32 = 20.0;
const MINUS_RADIUS: f32 = -20.0;

/// The position of the sun [soft 0x082a6c40 SI_lightSunGetPosition]:
/// `SI_lightSunGetPosition(light, out)`, the three floats of `out` written (the sun record is
/// at light + 0x16c: latitude +4, longitude +8, the time at +0x10). No light or no sun record:
/// (1, 1, 1). `localtime` null: `out` is left as it was.
///
/// The hour angle: A = f32((hour + min / 60 − 12) × 15) (register arithmetic); the longitude
/// l from `latlong2deg(+8)`, k = |l| / 15 truncated, the zone meridian taken off (l ≥ 0 or
/// NaN: l − 15k − 7.5; else l + 15k + 7.5, in the register) and stored as float B;
/// A = f32(A + B) then f32(A − eqTime), and the angle f32(0.0174532925 × A). The declination
/// is asin of the clamped product sin(f32(2π (day − 80) / 365.25)) × sin(0.4084…), the day
/// first made 365.25 + day when below 80 (stored as float), the product folded back at 1 and
/// at −1 as the original's two branches do. Then the altitude (asin) and the azimuth
/// (`fpatan`) from the latitude `latlong2deg(trunc(+4 × 0.0174532925))`, and the vector
/// ( sin(az) × 20 × cos(alt), sin(alt) × 20, cos(az) × −20 × cos(alt) ). Class: libm.
pub fn light_sun_get_position(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32, out: u32) {
    let sun = if light != 0 { m.u32(light + 0x16c) } else { 0 };
    if sun == 0 {
        for k in 0..3 {
            m.set_u32(out + 4 * k, 0x3f80_0000);
        }
        return;
    }
    let tm = o.localtime(m, sun + 0x10);
    if tm == 0 {
        return;
    }
    let hour = m.i32(tm + 8);
    let min = m.i32(tm + 4);
    // hour angle of the clock time
    let a: f32 = ((x(min) / x(SIXTY) + x(hour) - x(TWELVE)) * x(FIFTEEN_D)).to_f32();
    let lon = latlong2deg(m.i32(sun + 8));
    let k = (lon.abs() / x(FIFTEEN_F)).to_i32(Round::Zero);
    let b: f32 = if lon.lt(F80::ZERO) {
        // 0 > lon
        (lon + x(k.wrapping_mul(15)) + x(HALF_STEP)).to_f32()
    } else {
        (lon - x(k.wrapping_mul(15)) - x(HALF_STEP)).to_f32()
    };
    let eq = eq_time(m, tm);
    let a: f32 = (x(a) + x(b)).to_f32();
    let a: f32 = (x(a) - eq).to_f32();
    let hour_angle: f32 = (x(RADIANS) * x(a)).to_f32();
    // the declination
    let yday = x(m.i32(tm + 0x1c));
    let day: F80 = if x(EIGHTY).gt(yday) { x((yday + x(YEAR)).to_f32()) } else { yday };
    let arg: f32 = ((day - x(EIGHTY)) * x(TWO_PI) / x(YEAR)).to_f32();
    let s1 = trig::sinf(arg).to_f32();
    let prod: F80 = x(s1) * trig::sinf(TILT);
    let one = F80::ONE;
    let folded: F80 = if one.lt(prod) {
        one - (prod - one)
    } else if x(MINUS_ONE).gt(prod) {
        (one + prod) + x(MINUS_ONE)
    } else {
        prod
    };
    let dec = trig::asinf(folded.to_f32()).to_f32();
    let sd = trig::sinf(dec).to_f32();
    let cd = trig::cosf(dec).to_f32();
    let sh = trig::sinf(hour_angle).to_f32();
    let ch = trig::cosf(hour_angle).to_f32();
    // the latitude
    let lat_arg = (x(m.i32(sun + 4)) * x(RADIANS)).to_i32(Round::Zero);
    let lat = latlong2deg(lat_arg).to_f32();
    let sl = trig::sinf(lat).to_f32();
    let cl = trig::cosf(lat).to_f32();
    // the altitude
    let alt_sin: F80 = x(sd) * x(sl) + (x(cd) * x(cl)) * x(ch);
    let alt = trig::asinf(alt_sin.to_f32()).to_f32();
    let sy = trig::sinf(alt).to_f32();
    let cy = trig::cosf(alt).to_f32();
    // the azimuth
    let p1: F80 = x(cd) * x(ch);
    let yy: F80 = -x(cd) * x(sh);
    let xx: F80 = x(sd) * x(cl) - p1 * x(sl);
    let az = trig::fpatan(yy, xx).to_f32();
    m.set_f32(out, (trig::sinf(az) * x(RADIUS) * x(cy)).to_f32());
    m.set_f32(out + 4, (x(sy) * x(RADIUS)).to_f32());
    m.set_f32(out + 8, (trig::cosf(az) * x(MINUS_RADIUS) * x(cy)).to_f32());
}
