// SPDX-License-Identifier: MIT
//! Error-diffusion dithering of float rows to bytes, the four-corner gradation picture made
//! with it, and the renderer's output dither (finding 0014, "Gradation and dithering").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `pic_gradation.c` (0x085cf3e0–0x085d0053, 6
//! functions).
//!
//! The diffusion is the classical one: the rounding error of a pixel goes 7/16 to the next
//! pixel of the row and 3/16, 5/16 and 1/16 to the three pixels below it. What is this
//! program's own is where values are rounded: every value is a float in memory, the error is
//! kept in the x87 register, and each share is added and stored at once.

use crate::effect::Fault;
use crate::softpic::si_malloc;
use si_picture::Picture;
use si_x87::{Round, F80};

/// The diffusion state: the row being worked on and the errors collected for the next row,
/// three floats (red, green, blue) per pixel. In the original these are three globals
/// (`fSWidth`, `fSScan`, `fSAcc`) set up by [soft 0x085cf3e0 fsInit] and given back by
/// [soft 0x085cf444 fsRelease]; here the value is made by [`Diffusion::new`] and dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Diffusion {
    pub width: i32,
    /// The current row. The original does not clear it; every use fills it first.
    pub scan: Vec<f32>,
    /// The next row's share of the errors: zero at the start.
    pub acc: Vec<f32>,
}

/// C's `rint` on a float's value: to the nearest whole number, ties to even, as a float.
fn rint(v: f32) -> f32 {
    F80::from_f32(v).round_int(Round::Nearest).to_f32()
}

impl Diffusion {
    pub fn new(width: i32) -> Diffusion {
        let n = width.max(0) as usize * 3;
        Diffusion { width, scan: vec![0.0; n], acc: vec![0.0; n] }
    }

    /// The limits 0 and 255, the rounding and the spreading of the error for value `i`
    /// (pixel `i / 3`), shared by the two dithers; `clamp_first` is the one difference
    /// between them.
    fn settle(&mut self, i: usize, clamp_first: bool) {
        let (x, w) = ((i / 3) as i32, self.width);
        let limit = |v: f32| -> f32 {
            let r = F80::from_f32(v);
            if !(r.gt(F80::ZERO) || r.unordered(F80::ZERO)) {
                0.0
            } else if !(r.lt(F80::from_f64(255.0)) || r.unordered(F80::from_f64(255.0))) {
                255.0
            } else {
                v
            }
        };
        let before;
        if clamp_first {
            self.scan[i] = limit(self.scan[i]);
            before = self.scan[i];
            self.scan[i] = rint(before);
        } else {
            before = self.scan[i];
            self.scan[i] = limit(rint(before));
        }
        let err = F80::from_f32(before) - F80::from_f32(self.scan[i]);
        if err.abs().gt(F80::from_f64(0.0001)) {
            let share = |k: f64, to: f32| (err * F80::from_f64(k) + F80::from_f32(to)).to_f32();
            if x != w - 1 {
                self.scan[i + 3] = share(0.4375, self.scan[i + 3]);
            }
            if x != 0 {
                self.acc[i - 3] = share(0.1875, self.acc[i - 3]);
            }
            self.acc[i] = share(0.3125, self.acc[i]);
            if x != w - 1 {
                self.acc[i + 3] = share(0.0625, self.acc[i + 3]);
            }
        }
    }

    /// Dithers one row of three float channels in place to whole numbers 0…255
    /// [soft 0x085cf494 fsDitherFloat].
    ///
    /// Each value first takes the error collected for it from the row above (one float
    /// addition). Then, pixel by pixel and channel by channel: the value is rounded with
    /// `rint`; a result of 0 or less becomes 0, one of 255 or more 255; the error is the
    /// value before rounding minus the result, and if its size is above 0.0001 its shares
    /// go to the neighbours (none past either end of the row).
    pub fn dither_float(&mut self, r: &mut [f32], g: &mut [f32], b: &mut [f32]) -> Result<(), Fault> {
        let n = self.width.max(0) as usize;
        if r.len() < n || g.len() < n || b.len() < n {
            return Err(Fault::ShortPlane);
        }
        for i in 0..n {
            for (c, row) in [&*r, &*g, &*b].into_iter().enumerate() {
                self.scan[3 * i + c] = (F80::from_f32(row[i]) + F80::from_f32(self.acc[3 * i + c])).to_f32();
            }
            self.acc[3 * i..3 * i + 3].fill(0.0);
        }
        for i in 0..3 * n {
            self.settle(i, false);
        }
        for i in 0..n {
            r[i] = si_x87::fmov(self.scan[3 * i]);
            g[i] = si_x87::fmov(self.scan[3 * i + 1]);
            b[i] = si_x87::fmov(self.scan[3 * i + 2]);
        }
        Ok(())
    }
}

/// Fills the picture with a smooth gradation between four corner colours, dithered
/// [soft 0x085cf7c4 SI_pictureMakeGradation]. No function of the binary calls it.
///
/// `corners` are red, green, blue (0…255 as floats) of the top left, top right, bottom left
/// and bottom right corners, in the order of the original's twelve arguments. The picture
/// keeps its width and height, gets ratio 1.0, `fields` 3 and new red, green and blue
/// planes; alpha is left as it was.
///
/// With `v` the row's position (0 at the top, stepping by `float(1 / (height − 1))`, added
/// and rounded to float row by row) and `t` the column's (stepping by
/// `float(1 / (width − 1))`, **added in the register without rounding**):
/// left = `float((1 − v) × top left + v × bottom left)`, right likewise, pixel =
/// `float(left + t × (right − left))`, the difference taken in the register. Each row goes
/// through [`Diffusion::dither_float`], errors carrying from row to row, and is then
/// truncated to bytes.
pub fn make_gradation(p: &mut Picture, corners: [[f32; 3]; 4]) -> Result<(), Fault> {
    let (w, h) = (p.width as i32, p.height as i32);
    let dx = (F80::ONE / F80::from_i32(w - 1)).to_f32();
    let dy = (F80::ONE / F80::from_i32(h - 1)).to_f32();
    p.ratio = 1.0;
    p.fields = 3;
    let n = w * h;
    if n > 0 && n as usize > si_picture::MAX_ALLOC {
        return Err(Fault::BadSize);
    }
    for c in 0..3 {
        p.planes[c] = si_malloc(n);
    }
    let mut fs = Diffusion::new(w);
    let wu = w.max(0) as usize;
    let mut rows = [vec![0f32; wu], vec![0f32; wu], vec![0f32; wu]];
    let mut v = 0f32;
    let [tl, tr, bl, br] = corners;
    let f = F80::from_f32;
    let mut at = 0usize;
    for _ in 0..h.max(0) {
        let (vr, ur) = (f(v), F80::ONE - f(v));
        let left: [f32; 3] = [0, 1, 2].map(|c| (vr * f(bl[c]) + ur * f(tl[c])).to_f32());
        let right: [f32; 3] = [0, 1, 2].map(|c| (vr * f(br[c]) + ur * f(tr[c])).to_f32());
        v = (vr + f(dy)).to_f32();
        if w > 0 {
            let delta: [F80; 3] = [0, 1, 2].map(|c| f(right[c]) - f(left[c]));
            let mut t = F80::ZERO;
            for x in 0..wu {
                for c in 0..3 {
                    rows[c][x] = (t * delta[c] + f(left[c])).to_f32();
                }
                t = t + f(dx);
            }
        }
        let [r, g, b] = &mut rows;
        fs.dither_float(r, g, b)?;
        for x in 0..wu {
            for c in 0..3 {
                // a picture of this size always has its planes; the original does not test
                p.planes[c].as_mut().ok_or(Fault::NullPlane)?[at + x] = f(rows[c][x]).to_i32(Round::Zero) as u8;
            }
        }
        at += wu;
    }
    Ok(())
}

/// The C library's 48-bit generator as `drand48` steps it: state × 0x5DEECE66D + 0xB,
/// modulo 2⁴⁸; the value is the new state divided by 2⁴⁸.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drand48(pub u64);

impl Drand48 {
    /// The state `srand48(seed)` sets: the seed above the constant 0x330E.
    pub fn seeded(seed: u32) -> Drand48 {
        Drand48(((seed as u64) << 16) | 0x330e)
    }

    /// The state of a process that never seeded the generator.
    pub fn unseeded() -> Drand48 {
        Drand48(0x1234_abcd_330e)
    }

    pub fn next(&mut self) -> f64 {
        self.0 = (self.0.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb)) & 0xffff_ffff_ffff;
        self.0 as f64 / (1u64 << 48) as f64
    }
}

/// One pixel as the renderer hands a row to its output dither (the original's `SI_RGBAZO`,
/// 24 bytes): red, green, blue, alpha as floats 0…1, the depth, and a sixth word this
/// routine does not look at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgbazo {
    pub rgba: [f32; 4],
    pub depth: f32,
}

/// The renderer's output dither: the diffusion state, whether noise is added, and the
/// random generator the noise comes from.
///
/// Set up by [soft 0x085cfac4 SI_fsDitherInit]: the width, the noise switch, the generator
/// seeded with 0xd431 when noise is on (and left alone otherwise), and the two rows taken
/// from the renderer's own allocator with the error row cleared. (Read, not run in the
/// emulator: it needs the renderer's allocator.)
#[derive(Clone, Debug, PartialEq)]
pub struct OutputDither {
    pub rows: Diffusion,
    pub noise: bool,
    pub random: Drand48,
}

impl OutputDither {
    /// `random` is the generator's state before the call: [`Drand48::unseeded`] in a
    /// process that has not used it.
    pub fn new(noise: i16, width: i32, random: Drand48) -> OutputDither {
        OutputDither { rows: Diffusion::new(width), noise: noise != 0, random: if noise != 0 { Drand48::seeded(0xd431) } else { random } }
    }

    /// One rendered row to bytes [soft 0x085cfb60 SI_fsDitherScan].
    ///
    /// - Colours: `float(255 × value + error from the row above)`. With noise on, and only
    ///   for pixels whose depth is not exactly −100000 (the background), each channel then
    ///   gets `(drand48() − 0.5) × amplitude` added (three draws a pixel, red first), again
    ///   rounded to float. `amplitude` is the float global at 0x08790e38.
    /// - Then as [`Diffusion::dither_float`] with one difference: the value is limited to
    ///   0…255 **before** it is rounded, so the error is taken from the limited value.
    /// - Alpha is not dithered: trunc(255 × alpha), low byte. The depth is copied.
    pub fn scan(&mut self, input: &[Rgbazo], out: [&mut [u8]; 4], depth: &mut [f32], amplitude: f32) -> Result<(), Fault> {
        let n = self.rows.width.max(0) as usize;
        let [r, g, b, a] = out;
        if input.len() < n || r.len() < n || g.len() < n || b.len() < n || a.len() < n || depth.len() < n {
            return Err(Fault::ShortPlane);
        }
        let full = F80::from_f64(255.0);
        for (i, px) in input.iter().take(n).enumerate() {
            for c in 0..3 {
                self.rows.scan[3 * i + c] = (full * F80::from_f32(px.rgba[c]) + F80::from_f32(self.rows.acc[3 * i + c])).to_f32();
            }
            if self.noise && !F80::from_f32(f32::from_bits(0xc7c3_5000)).eq(F80::from_f32(px.depth)) {
                for c in 0..3 {
                    let d = F80::from_f64(self.random.next());
                    self.rows.scan[3 * i + c] = ((d - F80::from_f64(0.5)) * F80::from_f32(amplitude) + F80::from_f32(self.rows.scan[3 * i + c])).to_f32();
                }
            }
            self.rows.acc[3 * i..3 * i + 3].fill(0.0);
            a[i] = (full * F80::from_f32(px.rgba[3])).to_i32(Round::Zero) as u8;
            depth[i] = si_x87::fmov(px.depth);
        }
        for i in 0..3 * n {
            self.rows.settle(i, true);
        }
        for i in 0..n {
            r[i] = F80::from_f32(self.rows.scan[3 * i]).to_i32(Round::Zero) as u8;
            g[i] = F80::from_f32(self.rows.scan[3 * i + 1]).to_i32(Round::Zero) as u8;
            b[i] = F80::from_f32(self.rows.scan[3 * i + 2]).to_i32(Round::Zero) as u8;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_goes_to_four_neighbours() {
        let mut fs = Diffusion::new(3);
        let (mut r, mut g, mut b) = (vec![10.0f32, 10.5, 10.0], vec![0.0f32; 3], vec![300.0f32, -3.0, 0.0]);
        fs.dither_float(&mut r, &mut g, &mut b).unwrap();
        // 10.5 rounds to the even 10; 7/16 of its half goes to the right, which stays 10 and
        // passes its own 0.21875 on: below the middle pixel 0.5 × 5/16 + 0.21875 × 3/16
        assert_eq!(r, vec![10.0, 10.0, 10.0]);
        assert_eq!([fs.acc[0], fs.acc[3], fs.acc[6]], [0.09375, 0.19726563, 0.099609375]);
        assert_eq!([fs.acc[1], fs.acc[4], fs.acc[7]], [0.0; 3]);
        // limited to 0…255, and the error of the limit is spread too
        assert_eq!(b[0], 255.0);
    }

    #[test]
    fn one_colour_in_all_corners_gives_one_colour() {
        let mut p = Picture { width: 5, height: 3, ..Picture::default() };
        make_gradation(&mut p, [[10.0, 200.0, 99.0]; 4]).unwrap();
        assert_eq!((p.ratio, p.fields), (1.0, 3));
        assert_eq!(p.planes[0].as_deref(), Some(&[10u8; 15][..]));
        assert_eq!(p.planes[1].as_deref(), Some(&[200u8; 15][..]));
        assert!(p.planes[3].is_none());
    }

    #[test]
    fn a_ramp_from_left_to_right() {
        let mut p = Picture { width: 3, height: 1, ..Picture::default() };
        make_gradation(&mut p, [[0.0; 3], [100.0; 3], [0.0; 3], [100.0; 3]]).unwrap();
        assert_eq!(p.planes[0].as_deref(), Some(&[0u8, 50, 100][..]));
    }

    #[test]
    fn the_generator_is_the_c_librarys() {
        // the well-known first value after srand48(0)
        let mut r = Drand48::seeded(0);
        assert!((r.next() - 0.170828036106290).abs() < 1e-14);
    }

    #[test]
    fn the_output_dither_leaves_alpha_and_depth_alone() {
        let mut d = OutputDither::new(0, 2, Drand48::unseeded());
        let input = [Rgbazo { rgba: [0.5, 0.0, 1.0, 0.5], depth: -3.0 }, Rgbazo { rgba: [0.5, 2.0, -1.0, 1.0], depth: -100000.0 }];
        let (mut r, mut g, mut b, mut a, mut z) = ([0u8; 2], [0u8; 2], [0u8; 2], [0u8; 2], [0f32; 2]);
        d.scan(&input, [&mut r, &mut g, &mut b, &mut a], &mut z, 1.0).unwrap();
        assert_eq!(r, [128, 127]); // 127.5 rounds to 128; the half taken too much comes back
        assert_eq!((g, b), ([0, 255], [255, 0]));
        assert_eq!(a, [127, 255]);
        assert_eq!(z, [-3.0, -100000.0]);
    }
}
