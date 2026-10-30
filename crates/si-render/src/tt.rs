// SPDX-License-Identifier: MIT
//! The 3D textures: the original's `tt.c` (finding 0012, section 14): marble, wood and
//! cloud as functions of a point, built on the noise of [`crate::tt_noise`] and a ramp of
//! 1,024 colours.

use crate::tt_noise::{get_noise, init_noise, Noise};
use si_math::types::Matrix;
use si_x87::{trig, x, Round, F80};

/// 0x08657cd0, 0x08657ce0 (double): 2π, the factor from the texture's spacing to the
/// argument of the sine.
const TWO_PI: f64 = core::f64::consts::TAU;
/// 0x08657cd8, 0x08657d08, 0x08657d18 (double) and 0x08657cc8, 0x08657d00 (float).
const HALF: f64 = 0.5;
/// 0x08657ce8 (float, bits 0x3f4ccccd), 0x08657cf8 (double): wood's first amplitude.
const WOOD_AMP_F: f32 = 0.8;
const WOOD_AMP: f64 = 0.8;
/// 0x08657cec (float), 0x08657cf0 (double): wood's second frequency.
const WOOD_FREQ: f32 = 8.0;
/// 0x08657d20 (double): the cubic of the cloud, x² (3 − 2x).
const THREE: f64 = 3.0;
/// 0x08657d10, 0x08657d28 (float): the ramp's last index.
const RAMP_LAST: f32 = 1023.0;

/// The library calls of the three makers: `tanf` and `pow`. The defaults answer from
/// [`si_x87::trig`] (replayed in a check).
pub trait TtLib {
    fn tanf(&mut self, v: f32) -> F80 {
        trig::tanf(v)
    }
    fn pow(&mut self, b: f64, e: f64) -> F80 {
        trig::pow(b, e)
    }
    fn sinf(&mut self, v: f32) -> F80 {
        trig::sinf(v)
    }
}

/// The library as [`si_x87::trig`] has it.
pub struct TrigLib;
impl TtLib for TrigLib {}

/// `SI_Txt3d` (0x40a4 bytes, `SI_malloc`): a 3D texture ready to be evaluated.
#[derive(Clone, Debug, PartialEq)]
pub struct Txt3d {
    /// +0x00: 0 marble, 1 wood, 2 cloud.
    pub kind: i32,
    /// +0x04: 1: points go through the matrix first (it is not the identity).
    pub has_tform: i16,
    /// +0x08: a 4 × 4 matrix of which `SI_ttSetTform` writes the first three columns; the
    /// fourth (+0x14, +0x24, +0x34, +0x44) is what the allocator left.
    pub tform: Matrix,
    /// +0x48: 2π × spacing; +0x4c: tan(taper); +0x50: strength of the turbulence.
    pub scale: f32,
    pub taper: f32,
    pub strength: f32,
    /// +0x54: the number of noise terms; +0x58: per term its amplitude and its frequency
    /// (eight terms have room before the ramp).
    pub terms: i32,
    pub term: [[f32; 2]; 8],
    /// +0x98: the ramp, 1,024 colours of four floats.
    pub ramp: Vec<[f32; 4]>,
    /// +0x4098, +0x409c, +0x40a0: the ramp indices of the three inner colours.
    pub stops: [i32; 3],
}

impl Default for Txt3d {
    fn default() -> Txt3d {
        Txt3d { kind: 0, has_tform: 0, tform: [[0.0; 4]; 4], scale: 0.0, taper: 0.0, strength: 0.0, terms: 0, term: [[0.0; 2]; 8], ramp: vec![[0.0; 4]; 1024], stops: [0; 3] }
    }
}

/// The amplitudes and frequencies of marble and cloud: amplitude 0.5, frequency 1; each
/// further term has twice the frequency and the amplitude float(pow(2 × frequency, −power))
/// × 0.5 (rounded to float); at the end every amplitude is divided by their sum (summed in a
/// float). More than eight terms write into the ramp in the original; here that is refused.
fn octaves(t: &mut Txt3d, n: i32, power: f32, keep_frequency_wide: bool, lib: &mut dyn TtLib) {
    assert!(n <= 8, "{n} noise terms: the ninth pair is the ramp's first colour");
    let exponent = (-x(power)).to_f64();
    let mut amp = 0.5f32;
    let mut freq = F80::ONE;
    let mut total = 0.0f32;
    for i in 0..n.max(0) as usize {
        t.term[i] = [amp, freq.to_f32()];
        total = (x(total) + amp).to_f32();
        let twice = freq + freq;
        // marble keeps the frequency in a float between the terms, cloud in the register
        freq = if keep_frequency_wide { twice } else { x(twice.to_f32()) };
        let p = lib.pow(twice.to_f64(), exponent).to_f32();
        amp = (x(p) * HALF).to_f32();
    }
    for i in 0..n.max(0) as usize {
        t.term[i][0] = (x(t.term[i][0]) / total).to_f32();
    }
}

/// A marble [soft 0x08318890 SI_ttMakeMarble]: `SI_ttMakeMarble(spacing, taper, strength,
/// terms, power)`. Kind 0, no transformation; scale = float(2π × spacing); taper =
/// float(tanf(taper)); the terms as [`octaves`] describes; the noise table is filled if it
/// is not yet. Class: libm.
pub fn make_marble(noise: &mut Noise, spacing: f32, taper: f32, strength: f32, terms: i32, power: f32, lib: &mut dyn TtLib) -> Txt3d {
    let mut t = Txt3d { kind: 0, scale: (x(TWO_PI) * spacing).to_f32(), ..Txt3d::default() };
    t.taper = lib.tanf(taper).to_f32();
    t.strength = strength;
    t.terms = terms;
    octaves(&mut t, terms, power, false, lib);
    init_noise(noise);
    t
}

/// A wood [soft 0x08318994 SI_ttMakeWood]: `SI_ttMakeWood(spacing, taper, strength,
/// power)`. Kind 1; scale and taper as marble; two terms: frequencies 1 and 8, amplitudes
/// 0.8 and q = float(float(pow(8, −power)) × 0.8), both divided by (0.8 + q) (the sum in the
/// register). Class: libm.
pub fn make_wood(noise: &mut Noise, spacing: f32, taper: f32, strength: f32, power: f32, lib: &mut dyn TtLib) -> Txt3d {
    let mut t = Txt3d { kind: 1, scale: (x(TWO_PI) * spacing).to_f32(), ..Txt3d::default() };
    t.taper = lib.tanf(taper).to_f32();
    t.strength = strength;
    t.terms = 2;
    let p = lib.pow(WOOD_FREQ as f64, (-x(power)).to_f64()).to_f32();
    let q = (x(p) * WOOD_AMP).to_f32();
    let sum = x(WOOD_AMP_F) + q;
    t.term[0] = [(x(WOOD_AMP_F) / sum).to_f32(), 1.0];
    t.term[1] = [(x(q) / sum).to_f32(), WOOD_FREQ];
    init_noise(noise);
    t
}

/// A cloud [soft 0x08318a60 SI_ttMakeCloud]: `SI_ttMakeCloud(terms, power)`. Kind 2; the
/// terms as marble's, the frequency kept in the register from term to term; scale, taper
/// and strength are not set (what the allocator left; 0 here; a cloud does not read them).
/// Class: libm.
pub fn make_cloud(noise: &mut Noise, terms: i32, power: f32, lib: &mut dyn TtLib) -> Txt3d {
    let mut t = Txt3d { kind: 2, terms, ..Txt3d::default() };
    octaves(&mut t, terms, power, true, lib);
    init_noise(noise);
    t
}

/// Gives a texture its matrix [soft 0x08318b58 SI_ttSetTform]: `SI_ttSetTform(texture,
/// matrix)`: the first three columns of the four rows are copied, and the word at +0x04 is
/// 0 when those twelve floats are exactly the identity's (and none is a NaN), else 1. Class:
/// copy.
pub fn set_tform(t: &mut Txt3d, m: &Matrix) {
    let mut identity = true;
    for r in 0..4 {
        for c in 0..3 {
            t.tform[r][c] = m[r][c];
            let want = if r == c { 1.0f32 } else { 0.0 };
            identity &= x(m[r][c]).eq(x(want));
        }
    }
    t.has_tform = i16::from(!identity);
}

/// Fills the ramp [soft 0x08318c8c SI_ttSetColorMap]: `SI_ttSetColorMap(texture, colour 0 …
/// colour 4, f1, f2, f3)`. The three stops are trunc(f × 1023), kept at +0x4098 …; the five
/// colours stand at ramp indices 0, the three stops and 1023. Between two of them that are n
/// > 0 entries apart the entries are the first colour plus k times (difference / n): the
/// step stays in the register, the running colour is a float rounded at every addition; the
/// entry at the far end is the second colour itself. Stops out of order (n < 1) make the
/// writing position go back by −n entries and write nothing.
///
/// A position outside the 1,024 entries is a write outside the ramp (into the term table
/// before it, or behind the record); `Err` here. Class: model.
pub fn set_color_map(t: &mut Txt3d, colours: &[[f32; 4]; 5], f: [f32; 3]) -> Result<(), &'static str> {
    let stop = |v: f32| (x(v) * RAMP_LAST).to_i32(Round::Zero);
    t.stops = [stop(f[0]), stop(f[1]), stop(f[2])];
    let at = [0, t.stops[0], t.stops[1], t.stops[2], 0x3ff];
    let mut pos: i64 = 0;
    let put = |ramp: &mut Vec<[f32; 4]>, pos: i64, c: [f32; 4]| -> Result<(), &'static str> {
        *usize::try_from(pos).ok().and_then(|p| ramp.get_mut(p)).ok_or("SI_ttSetColorMap: a stop outside the ramp")? = c;
        Ok(())
    };
    for seg in 0..4 {
        let n = at[seg + 1].wrapping_sub(at[seg]);
        if n < 1 {
            pos += n as i64;
            continue;
        }
        let (from, to) = (colours[seg], colours[seg + 1]);
        let step = [0, 1, 2, 3].map(|c| (x(to[c]) - from[c]) / x(n));
        let mut cur = from;
        for _ in at[seg]..at[seg + 1] {
            put(&mut t.ramp, pos, cur)?;
            for c in 0..4 {
                cur[c] = (x(cur[c]) + step[c]).to_f32();
            }
            pos += 1;
        }
        put(&mut t.ramp, pos, to)?;
    }
    Ok(())
}

/// A colour of the ramp [soft 0x083192dc lookupColor]: `lookupColor(&result, ramp, value)`:
/// p = value × 1023 in the register, i = trunc(p), f = p − i, g = 1 − f rounded to float;
/// each component g × ramp[i] + f × ramp[i + 1] (ramp[1023] for i = 1023), rounded once. A
/// value outside 0 … 1 reads outside the ramp (a panic here). Class: model.
pub fn lookup_color(ramp: &[[f32; 4]], value: f32) -> [f32; 4] {
    let p = x(value) * RAMP_LAST;
    let i = p.to_i32(Round::Zero);
    let f = p - x(i);
    let g = (F80::ONE - f).to_f32();
    let j = if i == 0x3ff { 0x3ff } else { i.wrapping_add(1) };
    let entry = |k: i32| *usize::try_from(k).ok().and_then(|k| ramp.get(k)).unwrap_or_else(|| panic!("ramp index {k} outside the ramp"));
    let (lo, hi) = (entry(i), entry(j));
    [0, 1, 2, 3].map(|c| ((x(g) * lo[c]) + (f * hi[c])).to_f32())
}

/// What `SI_ttApplyTxt3d` prints for a kind that is none of the three (to standard error,
/// with the source file's name and a line number), before returning without a result.
pub const MSG_UNEXPECTED: &str = "Unexpected default in file";

/// The texture's colour at a point [soft 0x08318f78 SI_ttApplyTxt3d]:
/// `SI_ttApplyTxt3d(texture, &point, &colour)`. `None` for a kind that is not 0, 1 or 2 (the
/// result is not written).
///
/// The point goes through the matrix when the word at +0x04 is set (rows as vectors, the
/// translation last; each coordinate summed in the register and rounded once). Then:
///
/// - **marble**: t = Σ amplitude × s over the terms, s = 2n − 1 for a noise n above 0.5 and
///   (1 − n) − n otherwise, the noise taken at float(frequency × coordinate), the sum
///   rounded to float at every term; value = 0.5 − sinf(float(((t × strength) + ((y × taper)
///   + x)) × scale)) × 0.5.
/// - **wood**: two noises; value = 0.5 − sinf(float(((((n₂ × a₂) + n₁ × a₁) × strength) +
///   (√(z² + y²) + x × taper)) × scale)) × 0.5.
/// - **cloud**: s = Σ noise × amplitude in the register; value = float(((3 − s) − s) × (s ×
///   s)), held to 0 … 1.
///
/// The value, rounded to float, picks the colour ([`lookup_color`]). Class: libm (`sinf`);
/// cloud is model.
pub fn apply_txt3d(t: &Txt3d, noise: &Noise, p: [f32; 3], lib: &mut dyn TtLib) -> Option<[f32; 4]> {
    let m = &t.tform;
    let [px, py, pz] = if t.has_tform != 0 {
        [
            ((((x(m[1][0]) * p[1]) + (x(m[0][0]) * p[0])) + (x(m[2][0]) * p[2])) + m[3][0]).to_f32(),
            ((((x(m[0][1]) * p[0]) + (x(m[1][1]) * p[1])) + (x(m[2][1]) * p[2])) + m[3][1]).to_f32(),
            (((x(p[2]) * m[2][2]) + ((x(p[1]) * m[1][2]) + (x(p[0]) * m[0][2]))) + m[3][2]).to_f32(),
        ]
    } else {
        p
    };
    let at = |f: f32| get_noise(noise, (x(f) * px).to_f32(), (x(f) * py).to_f32(), (x(f) * pz).to_f32());
    let terms = t.terms.max(0) as usize;
    assert!(terms <= 8, "{terms} noise terms: read from the ramp in the original");
    let value = match t.kind {
        0 => {
            let mut turb = 0.0f32;
            for i in 0..terms {
                let n = at(t.term[i][1]);
                let s = if n.gt(x(HALF)) { (n + n) - F80::ONE } else { (F80::ONE - n) - n };
                turb = ((s * t.term[i][0]) + turb).to_f32();
            }
            let arg = (((x(turb) * t.strength) + ((x(py) * t.taper) + px)) * t.scale).to_f32();
            (x(HALF) - (lib.sinf(arg) * HALF)).to_f32()
        }
        1 => {
            let first = at(t.term[0][1]) * t.term[0][0];
            let second = at(t.term[1][1]);
            let arg = (((((second * t.term[1][0]) + first) * t.strength) + (((x(pz) * pz) + (x(py) * py)).sqrt() + (x(px) * t.taper))) * t.scale).to_f32();
            (x(HALF) - (lib.sinf(arg) * HALF)).to_f32()
        }
        2 => {
            let mut s = x(0.0f32);
            for i in 0..terms {
                s = (at(t.term[i][1]) * t.term[i][0]) + s;
            }
            let v = (((x(THREE) - s) - s) * (s * s)).to_f32();
            if x(v).gt(x(1.0f32)) {
                1.0
            } else if x(0.0f32).gt(x(v)) {
                0.0
            } else {
                v
            }
        }
        _ => return None,
    };
    Some(lookup_color(&t.ramp, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_reaches_every_stop() {
        let mut t = Txt3d::default();
        let c = [[0.0; 4], [0.25; 4], [0.5; 4], [0.75; 4], [1.0; 4]];
        set_color_map(&mut t, &c, [0.25, 0.5, 0.75]).unwrap();
        assert_eq!(t.stops, [255, 511, 767]);
        assert_eq!(t.ramp[0], [0.0; 4]);
        assert_eq!(t.ramp[255], [0.25; 4]);
        assert_eq!(t.ramp[511], [0.5; 4]);
        assert_eq!(t.ramp[1023], [1.0; 4]);
        assert!(t.ramp[100][0] > t.ramp[99][0]);
        assert_eq!(lookup_color(&t.ramp, 1.0), [1.0; 4]);
        assert_eq!(lookup_color(&t.ramp, 0.0), [0.0; 4]);
        // stops out of order: the position goes back, nothing is written there
        assert!(set_color_map(&mut t, &c, [0.5, 0.25, 0.75]).is_ok());
        // a stop beyond the ramp is a write outside it
        assert!(set_color_map(&mut t, &c, [0.25, 0.5, 2.0]).is_err());
    }

    #[test]
    fn the_identity_needs_no_transformation() {
        let mut t = Txt3d::default();
        let mut m = si_math::types::IDENTITY;
        set_tform(&mut t, &m);
        assert_eq!(t.has_tform, 0);
        m[3][1] = 0.5;
        set_tform(&mut t, &m);
        assert_eq!(t.has_tform, 1);
        // the fourth column is not looked at
        m[3][1] = 0.0;
        m[0][3] = 7.0;
        set_tform(&mut t, &m);
        assert_eq!(t.has_tform, 0);
    }

    #[test]
    fn makers() {
        let mut n = Noise::default();
        let t = make_marble(&mut n, 1.0, 0.0, 1.0, 3, 1.0, &mut TrigLib);
        assert_eq!(n.first_call, 0);
        assert_eq!(t.term[0][1], 1.0);
        assert_eq!(t.term[1][1], 2.0);
        let sum: f32 = (0..3).map(|i| t.term[i][0]).sum();
        assert!((sum - 1.0).abs() < 1e-6);
        let w = make_wood(&mut n, 1.0, 0.0, 1.0, 1.0, &mut TrigLib);
        assert!((w.term[0][0] + w.term[1][0] - 1.0).abs() < 1e-6);
        assert_eq!(w.term[1][1], 8.0);
        let c = make_cloud(&mut n, 2, 1.0, &mut TrigLib);
        assert_eq!(c.kind, 2);
        let mut cl = c.clone();
        set_color_map(&mut cl, &[[0.0; 4], [0.25; 4], [0.5; 4], [0.75; 4], [1.0; 4]], [0.25, 0.5, 0.75]).unwrap();
        let v = apply_txt3d(&cl, &n, [0.3, 0.4, 0.5], &mut TrigLib).unwrap();
        assert!((0.0..=1.0).contains(&v[0]));
    }
}
