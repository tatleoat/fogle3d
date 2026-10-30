// SPDX-License-Identifier: MIT
//! Output of finished lines and the renderer's utilities: the original's `renutil.c`
//! (finding 0012, "From samples to bytes"). Ported here: `SI_Output` and the texture
//! look-up `si_BlookUp`. The message and exit functions and the tree statistics are
//! described in the finding.

use crate::materials::TexPixels;
use crate::shade::ShadeState;
use crate::types::Rgbazo;
use si_x87::{x, Round, F80};

/// 0x086542f0, double: a channel value of 1 is this many units of a byte.
const BYTE_SCALE: f64 = 255.0;

/// Where `SI_Output` sends a finished line. Each method stands for one callee that is not
/// part of this port.
pub trait LineSink {
    /// `SI_fsDitherScan(samples, red, green, blue, alpha, depth)` (dither on, 0x08790e30):
    /// fills the byte and depth buffers itself, instead of the plain conversion.
    fn dither_scan(&mut self, samples: &[Rgbazo], red: &mut [u8], green: &mut [u8], blue: &mut [u8], alpha: &mut [u8], depth: &mut [f32]);
    /// `si_incrustration(width)` (when `SI_incrust` is set): stamps text into the line.
    fn incrustation(&mut self, line: &mut OutputLine, width: i32);
    /// `SI_rleWrite(line number, width, red, green, blue, alpha)` (picture file on,
    /// 0x08790d5a).
    fn rle_write(&mut self, y: i32, width: i32, line: &OutputLine);
    /// `SI_byteswap_fwrite(depth, 4, picture width, file)` (depth file on, 0x08790e2c).
    fn depth_write(&mut self, depth: &[f32]);
    /// `SI_fbWrite(line number)` (display on, 0x08790d58).
    fn fb_write(&mut self, y: i16);
    /// `SI_displayStatus` with the text "frame %d, line %d done" (progress on, 0x08790dcc),
    /// every `SI_reportFrequency` lines.
    fn status(&mut self, text: &str);
}

/// The four byte lines at 0x08790b54 to 0x08790b60 that go to the file and the screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputLine {
    pub red: Vec<u8>,
    pub green: Vec<u8>,
    pub blue: Vec<u8>,
    pub alpha: Vec<u8>,
}

/// The globals `SI_Output` reads and writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputState {
    /// 0x087907a0, 0x087907a4: first and last pixel column that was rendered.
    pub x0: i32,
    pub x1: i32,
    /// 0x08790b40 to 0x08790b4c: byte lines of the whole picture width + 10; 0x08790b50: the
    /// depth line.
    pub red: Vec<u8>,
    pub green: Vec<u8>,
    pub blue: Vec<u8>,
    pub alpha: Vec<u8>,
    pub depth: Vec<f32>,
    /// 0x08790b54..: the lines handed on.
    pub line: OutputLine,
    /// 0x08790b3c: the number of the line being written; counted down by each call.
    pub y: i16,
    /// 0x0879117a: picture width.
    pub width: i16,
    /// 0x08790dda: current frame (for the progress text).
    pub frame: i16,
    /// 0x08790e30 dither, `SI_incrust`, 0x08790d5a file, 0x08790e2c depth file, 0x08790d58
    /// display, 0x08790dcc progress, `SI_reportFrequency`.
    pub dither: bool,
    pub incrust: bool,
    pub to_file: bool,
    pub depth_file: bool,
    pub display: bool,
    pub progress: bool,
    pub report_frequency: i16,
}

/// One channel value as a byte: the low eight bits of `(int)(255.0 × value)`, the product
/// formed in the register and truncated toward zero. No clamping: the shading has already
/// held the value to 0..1, and anything else wraps. Kernel of `SI_Output`.
pub fn channel_byte(v: f32) -> u8 {
    (x(BYTE_SCALE) * v).to_i32(Round::Zero) as u8
}

/// Turns one line of pixel values into bytes and sends it on [soft 0x08260f88 SI_Output]:
/// `SI_Output(samples, x offset, y offset, width, depth file)`.
///
/// For the columns `x0..=x1`: red, green, blue and alpha by [`channel_byte`] and the depth
/// copied, or all five by the dither routine. Then `width` bytes from column `x_offset` of
/// each byte line are copied to the outgoing lines, and in this order: the incrustation, the
/// picture file, the depth file, the display (line number less `y_offset`), the progress
/// text (when the line number is a multiple of the report frequency). Last, the line number
/// goes down by one. Class: model (one product, truncated).
pub fn output(st: &mut OutputState, sink: &mut dyn LineSink, samples: &[Rgbazo], x_offset: i32, y_offset: i32, width: i32) {
    let (a, b) = (st.x0 as usize, st.x1 as usize);
    if st.dither {
        sink.dither_scan(&samples[a..], &mut st.red[a..], &mut st.green[a..], &mut st.blue[a..], &mut st.alpha[a..], &mut st.depth[a..]);
    } else if st.x0 <= st.x1 {
        for i in a..=b {
            let s = &samples[i];
            st.red[i] = channel_byte(s.r);
            st.green[i] = channel_byte(s.g);
            st.blue[i] = channel_byte(s.b);
            st.alpha[i] = channel_byte(s.a);
            st.depth[i] = s.z;
        }
    }
    copy_out(st, x_offset, width);
    if st.incrust {
        sink.incrustation(&mut st.line, width);
    }
    if st.to_file {
        sink.rle_write(st.y as i32, width, &st.line);
    }
    if st.depth_file {
        sink.depth_write(&st.depth[..st.width as usize]);
    }
    if st.display {
        sink.fb_write(st.y.wrapping_sub(y_offset as i16));
    }
    if st.progress && st.y as i32 % st.report_frequency as i32 == 0 {
        sink.status(&format!("frame {}, line {} done", st.frame, st.y));
    }
    st.y = st.y.wrapping_sub(1);
}

/// The four `memcpy` of `SI_Output`: `width` bytes from column `x_offset` of each byte
/// line to its outgoing line, red, green, blue, alpha in that order.
///
/// `initBuffers` takes the byte lines and the outgoing lines from chunk memory one after
/// the other, each `padSize(w)` bytes apart (w = picture width + 10, `padSize` rounds up to
/// four): red, green, blue, alpha, the depth line (4w), then the four outgoing lines. A
/// crop wider than w less its offset (`srXOffset` + `srXRes` > w, which a sub-region whose
/// pixel size was not brought up to the picture's gives, `SI_updateSubregionValues` not
/// having run) makes the original read behind each byte line, into the next one, and write
/// behind each outgoing line, into the next one, which the next copy then writes over. Here
/// both run on that layout, so the lines handed on are what the original hands on:
///
/// - read: behind red come its padding and green, and so on; behind alpha the depth line's
///   bytes. The padding bytes are never written by the original (they hold what the arena
///   held: zero in a fresh block of the C library's); 0 here. Behind the depth line lie the
///   outgoing lines themselves, being written: not reproduced (0 here; no corpus scene
///   reaches it);
/// - write: the bytes the alpha copy puts behind its outgoing line land in the next piece of
///   chunk memory (whatever `SI_update` took first): kept here in the outgoing line, which
///   grows to `width`, and not written anywhere else.
///
/// A negative offset reads before the red line (the line of samples): not reproduced; the
/// offset is taken as 0. Not a function of the original: part of [`output`].
fn copy_out(st: &mut OutputState, x_offset: i32, width: i32) {
    let n = width.max(0) as usize;
    let from = x_offset.max(0) as usize;
    let w = st.red.len();
    if from + n <= w {
        st.line.red[..n].copy_from_slice(&st.red[from..from + n]);
        st.line.green[..n].copy_from_slice(&st.green[from..from + n]);
        st.line.blue[..n].copy_from_slice(&st.blue[from..from + n]);
        st.line.alpha[..n].copy_from_slice(&st.alpha[from..from + n]);
        return;
    }
    let pad = crate::mem_chunk::pad_size(w as i32) as usize;
    // the byte lines and the depth line as they lie in chunk memory
    let mut source = vec![0u8; 4 * pad + 4 * w];
    for (c, plane) in [&st.red, &st.green, &st.blue, &st.alpha].into_iter().enumerate() {
        source[c * pad..c * pad + w].copy_from_slice(plane);
    }
    for (k, d) in st.depth.iter().take(w).enumerate() {
        source[4 * pad + 4 * k..4 * pad + 4 * k + 4].copy_from_slice(&d.to_le_bytes());
    }
    let byte = |i: usize| source.get(i).copied().unwrap_or(0);
    // the outgoing lines as they lie in chunk memory, written in order
    let mut out = vec![0u8; 3 * pad + n.max(pad)];
    for (c, line) in [&st.line.red, &st.line.green, &st.line.blue, &st.line.alpha].into_iter().enumerate() {
        let k = line.len().min(pad);
        out[c * pad..c * pad + k].copy_from_slice(&line[..k]);
    }
    for c in 0..4 {
        for k in 0..n {
            out[c * pad + k] = byte(c * pad + from + k);
        }
    }
    for (c, line) in [&mut st.line.red, &mut st.line.green, &mut st.line.blue, &mut st.line.alpha].into_iter().enumerate() {
        if line.len() < n {
            line.resize(n, 0);
        }
        line[..n].copy_from_slice(&out[c * pad..c * pad + n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crop wider than the byte lines: the copies read on into the next line and write
    /// on into the next outgoing line, as the pieces lie in chunk memory.
    #[test]
    fn a_crop_wider_than_the_lines_reads_the_next_line() {
        let plane = |b: u8| (0..6).map(|k| b + k).collect::<Vec<u8>>();
        let mut st = OutputState { red: plane(1), green: plane(11), blue: plane(21), alpha: plane(31), depth: vec![f32::from_bits(0x0403_0201); 6], line: OutputLine { red: vec![0; 6], green: vec![0; 6], blue: vec![0; 6], alpha: vec![0; 6] }, ..OutputState::default() };
        copy_out(&mut st, 0, 10);
        // six bytes of the line, two of padding, then the next line's first two
        assert_eq!(st.line.red, [1, 2, 3, 4, 5, 6, 0, 0, 11, 12]);
        assert_eq!(st.line.green, [11, 12, 13, 14, 15, 16, 0, 0, 21, 22]);
        assert_eq!(st.line.blue, [21, 22, 23, 24, 25, 26, 0, 0, 31, 32]);
        // behind alpha: the depth line's bytes
        assert_eq!(st.line.alpha, [31, 32, 33, 34, 35, 36, 0, 0, 1, 2]);
        // inside the lines nothing changes
        let mut st2 = OutputState { red: plane(1), green: plane(11), blue: plane(21), alpha: plane(31), depth: vec![0.0; 6], line: OutputLine { red: vec![0; 6], green: vec![0; 6], blue: vec![0; 6], alpha: vec![0; 6] }, ..OutputState::default() };
        copy_out(&mut st2, 2, 4);
        assert_eq!(&st2.line.red[..4], &[3, 4, 5, 6]);
    }

    #[derive(Default)]
    struct Sink(Vec<String>);
    impl LineSink for Sink {
        fn dither_scan(&mut self, _: &[Rgbazo], red: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [u8], _: &mut [f32]) {
            red[0] = 77;
            self.0.push("dither".into());
        }
        fn incrustation(&mut self, _: &mut OutputLine, w: i32) {
            self.0.push(format!("incrust {w}"));
        }
        fn rle_write(&mut self, y: i32, w: i32, line: &OutputLine) {
            self.0.push(format!("rle {y} {w} {:?}", &line.red[..w as usize]));
        }
        fn depth_write(&mut self, d: &[f32]) {
            self.0.push(format!("depth {d:?}"));
        }
        fn fb_write(&mut self, y: i16) {
            self.0.push(format!("fb {y}"));
        }
        fn status(&mut self, t: &str) {
            self.0.push(t.into());
        }
    }

    #[test]
    fn bytes_truncate() {
        assert_eq!(channel_byte(1.0), 255);
        assert_eq!(channel_byte(0.0), 0);
        // 0.5 × 255 = 127.5 → 127; the float just below 1 → 254
        assert_eq!(channel_byte(0.5), 127);
        assert_eq!(channel_byte(f32::from_bits(0x3f7f_ffff)), 254);
        // nothing clamps: 2.0 → 510 → 254; −0.5 → −127 → 129
        assert_eq!(channel_byte(2.0), 254);
        assert_eq!(channel_byte(-0.5), 129);
    }

    #[test]
    fn output_order_and_line_counter() {
        let mut st = OutputState { x0: 1, x1: 2, y: 20, width: 4, frame: 7, to_file: true, depth_file: true, display: true, progress: true, incrust: true, report_frequency: 10, ..OutputState::default() };
        for v in [&mut st.red, &mut st.green, &mut st.blue, &mut st.alpha] {
            *v = vec![9; 14];
        }
        st.depth = vec![0.0; 14];
        st.line = OutputLine { red: vec![0; 14], green: vec![0; 14], blue: vec![0; 14], alpha: vec![0; 14] };
        let px = |r: f32| Rgbazo { r, g: 0.0, b: 0.0, a: 1.0, z: -3.0, o: 0.0 };
        let samples = vec![px(0.0), px(1.0), px(0.5), px(0.25)];
        let mut sink = Sink::default();
        output(&mut st, &mut sink, &samples, 0, 5, 4);
        // only columns 1 and 2 are converted; column 0 and 3 keep what the buffer held
        assert_eq!(sink.0, vec!["incrust 4", "rle 20 4 [9, 255, 127, 9]", "depth [0.0, -3.0, -3.0, 0.0]", "fb 15", "frame 7, line 20 done"]);
        assert_eq!(st.y, 19);
        st.dither = true;
        st.progress = false;
        let mut sink = Sink::default();
        output(&mut st, &mut sink, &samples, 1, 0, 2);
        assert_eq!(sink.0[0], "dither");
        assert_eq!(sink.0[2], "rle 19 2 [77, 127]");
    }
}

// --------------------------------------------------------------------------------- si_BlookUp

/// 0x086542f8, 0x08654300 (double): a pixel's middle, and the same one pixel on.
const HALF: f64 = 0.5;
const ONE_AND_HALF: f64 = 1.5;

/// How many texture pixels were asked for outside their block (see [`blookup`]).
pub static WILD_TEXELS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One pixel of a texture as five numbers for the register (red, green, blue, alpha,
/// intensity). A pixel outside the block is a read outside the original's allocation: what
/// the original gets there is whatever lies behind the block. Here it is five zeros, and
/// [`WILD_TEXELS`] counts it (ninth pass; it was a panic before, which ended a frame for
/// one sample).
fn texel(p: &TexPixels, index: i32) -> [i32; 5] {
    match p.at(index) {
        Some(b) => b.map(|b| b as i32),
        None => {
            WILD_TEXELS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            [0; 5]
        }
    }
}

/// A 2D map's picture at (u, v) [soft 0x082614d8 si_BlookUp]: `si_BlookUp(u, v, map,
/// interpolate, &sample, bump, &sample_u, &sample_v)`. `interpolate` is the map's word at
/// +0x2c as every caller passes it. Returns the sample (five floats, 0 … 255: red, green,
/// blue, alpha, intensity) and, with `bump`, the alpha and the intensity (members 3 and 4;
/// the other three are not written) of the sample one pixel on in u and of the one one
/// pixel on in v.
///
/// **Picture coordinates.** x = (u − offset) × pixels per u, y = (v − offset) × pixels per
/// v, in the register: offsets +0x68 and +0x6c, or their fractions +0x70 and +0x74 on a
/// surface closed in that direction (+0x102, +0x104); scales +0x60 and +0x64. A negative
/// extent (+0x78 for u, +0x7a for v) turns the coordinate into (1 − c) − 1, **rounded to
/// float**. With u and v exchanged (+0xfc) the u value is the picture's y and the v value
/// its x. ix = trunc(x), iy = trunc(y) (towards zero, from the unrounded values).
///
/// **Nearest pixel** (`interpolate` 0). ix is taken modulo the width, iy modulo the height
/// (C's remainder, then + size when negative). With alternation in u (+0xfe) an odd
/// quotient ix / width mirrors: ix = (width − ix) − 1; with alternation in v (+0x100) an
/// **even** quotient mirrors. The pixel is the sample. The bump neighbours are the next
/// pixel of the row and the same pixel of the next row (without alternation: straight from
/// the block, whose border holds the wrapped pixels); with alternation the neighbour steps
/// against the mirroring and stays on the picture's last (or, mirrored, first) pixel.
///
/// **Interpolation** (`interpolate` not 0). fx = x − ix. fx < 0.5: a = 0.5 − fx and ix − 1;
/// else a = 1.5 − fx; a rounded to float; b the same from y. The four pixels (ix, iy),
/// (ix + 1, iy), (ix, iy + 1), (ix + 1, iy + 1) (wrapped or mirrored as above) are weighted
/// by ab, b − ab, a − ab (all three kept in the register) and ((1 − a) − b) + ab (rounded to
/// float), summed in that order, each channel rounded once. The bump neighbours are the
/// same sum one pixel on.
///
/// **Outside the block.** With alternation a negative picture coordinate is mirrored to a
/// row or column beyond the picture's border (the remainder of a negative number is
/// negative, and the mirror of −k is size + k − 1): the original reads the memory behind
/// its block there (seen in whole frames in the emulator, ninth pass: 6 of 1,000 scenes).
/// See [`WILD_TEXELS`].
///
/// The original reads the picture through the map's record (+0x24) without a null test and
/// divides by its width and height: a map without a loaded picture, or a picture of width
/// or height 0, is a fault there and a panic here. Class: model.
pub fn blookup(st: &ShadeState, u: f32, v: f32, map: usize, interpolate: i16, bump: bool) -> [[f32; 5]; 3] {
    let m = &st.maps[map];
    let g = &st.pictures[m.glb.expect("si_BlookUp: a map without a picture record")];
    let pix = g.pixels.as_ref().expect("si_BlookUp: a picture that was not read");
    let (w, h) = (g.width as i32, g.height as i32);
    let (alt_u, alt_v) = (m.alt_u != 0, m.alt_v != 0);
    let from_u = (x(u) - if m.band_u != 0 { m.uoff_frac } else { m.uoff }) * m.pix_u;
    let from_v = (x(v) - if m.band_v != 0 { m.voff_frac } else { m.voff }) * m.pix_v;
    let turn = |c: F80, on: i16| if on != 0 { x(((F80::ONE - c) - F80::ONE).to_f32()) } else { c };
    let (from_u, from_v) = (turn(from_u, m.neg_u), turn(from_v, m.neg_v));
    let (px, py) = if m.swap_uv != 0 { (from_v, from_u) } else { (from_u, from_v) };
    let mut ix = px.to_i32(Round::Zero);
    let mut iy = py.to_i32(Round::Zero);
    let stride = w.wrapping_add(3);
    let neg = |c: i32, size: i32| if c < 0 { c.wrapping_add(size) } else { c };
    let mut out = [[0.0f32; 5]; 3];

    if interpolate == 0 {
        let (mut flip_x, mut flip_y) = (false, false);
        let (mut step_x, mut step_y) = (1i32, 1i32);
        if !alt_u {
            ix %= w;
        } else {
            let q = ix / w;
            ix %= w;
            if q & 1 != 0 {
                flip_x = true;
                step_x = -1;
                ix = (w - ix) - 1;
            }
        }
        if !alt_v {
            iy %= h;
        } else {
            let q = iy / h;
            iy %= h;
            if q & 1 == 0 {
                flip_y = true;
                step_y = -1;
                iy = (h - iy) - 1;
            }
        }
        iy = neg(iy, h);
        ix = neg(ix, w);
        let row = stride.wrapping_mul(iy);
        let index = ix.wrapping_add(row);
        out[0] = texel(pix, index).map(|c| c as f32);
        if !bump {
            return out;
        }
        if !alt_u && !alt_v {
            let (du, dv) = (texel(pix, index + 1), texel(pix, index + stride));
            out[1][3] = du[3] as f32;
            out[1][4] = du[4] as f32;
            out[2][3] = dv[3] as f32;
            out[2][4] = dv[4] as f32;
            return out;
        }
        let mut ix2 = if !alt_u {
            ix.wrapping_add(step_x) % w
        } else if flip_x {
            if ix != 0 { ix.wrapping_add(step_x) % w } else { 0 }
        } else if ix == w - 1 {
            ix
        } else {
            ix.wrapping_add(step_x) % w
        };
        if !alt_v {
            iy = neg(iy.wrapping_add(step_y) % h, h);
        } else if !flip_y {
            if iy != h - 1 {
                iy = iy.wrapping_add(step_y) % h;
            }
            iy = neg(iy, h);
        } else if iy != 0 {
            iy = neg(iy.wrapping_add(step_y) % h, h);
        }
        ix = neg(ix, w);
        ix2 = neg(ix2, w);
        let du = texel(pix, row.wrapping_add(ix2));
        let dv = texel(pix, ix.wrapping_add(iy.wrapping_mul(stride)));
        out[1][3] = du[3] as f32;
        out[1][4] = du[4] as f32;
        out[2][3] = dv[3] as f32;
        out[2][4] = dv[4] as f32;
        return out;
    }

    // the two weights
    let fx = px - x(ix);
    let fy = py - x(iy);
    let a = if x(HALF).gt(fx) {
        ix = ix.wrapping_sub(1);
        (x(HALF) - fx).to_f32()
    } else {
        (x(ONE_AND_HALF) - fx).to_f32()
    };
    let b = if x(HALF).gt(fy) {
        iy = iy.wrapping_sub(1);
        (x(HALF) - fy).to_f32()
    } else {
        (x(ONE_AND_HALF) - fy).to_f32()
    };
    let w00 = x(a) * b;
    let w11 = (((F80::ONE - a) - b) + w00).to_f32();
    let w10 = x(b) - w00;
    let w01 = x(a) - w00;
    let sum = |c00: i32, c10: i32, c01: i32, c11: i32| ((((x(c00) * w00) + (x(c10) * w10)) + (x(c01) * w01)) + (x(c11) * w11)).to_f32();

    if !alt_u && !alt_v {
        ix %= w;
        iy %= h;
        iy = neg(iy, h);
        ix = neg(ix, w);
        let index = stride.wrapping_mul(iy).wrapping_add(ix);
        let (c00, c10) = (texel(pix, index), texel(pix, index + 1));
        let (c01, c11) = (texel(pix, index + stride), texel(pix, index + stride + 1));
        for k in 0..5 {
            out[0][k] = sum(c00[k], c10[k], c01[k], c11[k]);
        }
        if !bump {
            return out;
        }
        let (e10, e11) = (texel(pix, index + 2), texel(pix, index + stride + 2));
        out[1][4] = sum(c10[4], e10[4], c11[4], e11[4]);
        out[1][3] = sum(c10[3], e10[3], c11[3], e11[3]);
        let below = texel(pix, index + stride);
        let (f01, f11) = (texel(pix, index + 2 * stride), texel(pix, index + 2 * stride + 1));
        out[2][4] = sum(below[4], c11[4], f01[4], f11[4]);
        out[2][3] = ((x(w11) * x(f11[3])) + ((w01 * x(f01[3])) + ((w10 * x(c11[3])) + (w00 * x(below[3]))))).to_f32();
        return out;
    }

    let (mut flip_x, mut flip_y) = (false, false);
    let (mut step_x, mut step_y) = (1i32, 1i32);
    if !alt_u {
        ix %= w;
    } else {
        let q = ix / w;
        ix %= w;
        if q & 1 != 0 {
            flip_x = true;
            step_x = -1;
            ix = (w - ix) - 1;
        }
    }
    if !alt_v {
        iy %= h;
    } else {
        let q = iy / h;
        iy %= h;
        if q & 1 == 0 {
            flip_y = true;
            step_y = -1;
            iy = (h - iy) - 1;
        }
    }
    let mut ix1 = if !alt_u {
        ix.wrapping_add(step_x) % w
    } else if flip_x {
        if ix != 0 { ix.wrapping_add(step_x) % w } else { 0 }
    } else if ix != w - 1 {
        ix.wrapping_add(step_x) % w
    } else {
        ix
    };
    let mut iy1 = if !alt_v {
        iy.wrapping_add(step_y) % h
    } else if flip_y {
        if iy != 0 { iy.wrapping_add(step_y) % h } else { 0 }
    } else if iy != h - 1 {
        iy.wrapping_add(step_y) % h
    } else {
        iy
    };
    iy = neg(iy, h);
    iy1 = neg(iy1, h);
    ix = neg(ix, w);
    ix1 = neg(ix1, w);
    let row0 = iy.wrapping_mul(stride);
    let row1 = stride.wrapping_mul(iy1);
    let (c00, c10) = (texel(pix, ix.wrapping_add(row0)), texel(pix, ix1.wrapping_add(row0)));
    let (c01, c11) = (texel(pix, ix.wrapping_add(row1)), texel(pix, ix1.wrapping_add(row1)));
    for k in 0..5 {
        out[0][k] = sum(c00[k], c10[k], c01[k], c11[k]);
    }
    if !bump {
        return out;
    }
    let step2 = |c: i32, step: i32, size: i32| c.wrapping_add(step.wrapping_mul(2)) % size;
    let mut ix2 = if !alt_u {
        step2(ix, step_x, w)
    } else if flip_x {
        if ix == 1 {
            0
        } else if ix != 0 {
            step2(ix, step_x, w)
        } else {
            1
        }
    } else if ix == w - 2 {
        ix + 1
    } else if ix == w - 1 {
        w - 2
    } else {
        step2(ix, step_x, w)
    };
    let iy2 = if !alt_v {
        neg(step2(iy, step_y, h), h)
    } else if !flip_y {
        if iy == h - 2 {
            neg(iy + 1, h)
        } else if iy == h - 1 {
            neg(h - 2, h)
        } else {
            neg(step2(iy, step_y, h), h)
        }
    } else if iy == 1 {
        0
    } else if iy != 0 {
        neg(step2(iy, step_y, h), h)
    } else {
        1
    };
    ix = neg(ix, w);
    ix1 = neg(ix1, w);
    ix2 = neg(ix2, w);
    let row2 = stride.wrapping_mul(iy2);
    let (e10, e11) = (texel(pix, ix2.wrapping_add(row0)), texel(pix, ix2.wrapping_add(row1)));
    out[1][4] = sum(c10[4], e10[4], c11[4], e11[4]);
    out[1][3] = sum(c10[3], e10[3], c11[3], e11[3]);
    let below = texel(pix, ix.wrapping_add(row1));
    let (f01, f11) = (texel(pix, ix.wrapping_add(row2)), texel(pix, row2.wrapping_add(ix1)));
    out[2][4] = sum(below[4], c11[4], f01[4], f11[4]);
    out[2][3] = ((x(w11) * x(f11[3])) + ((w01 * x(f01[3])) + ((w10 * x(c11[3])) + (w00 * x(below[3]))))).to_f32();
    out
}
