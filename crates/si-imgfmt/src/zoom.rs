// SPDX-License-Identifier: MIT
//! Resizing from one picture file to another: the filtered zoom, and the two whole-factor
//! resizers that work a row at a time (finding 0014, "Resizing").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `size.c` (the in-memory resizers of the same file are
//! in [`crate::size`]).
//!
//! The filtered zoom is the well-known two-pass design: a table of weights per destination
//! column, the same per destination row, rows filtered horizontally as they are read and
//! kept in a ring until the vertical filter has all it needs. Every constant, the order of
//! the arithmetic and the rounding points are this binary's and are written out below.
//!
//! Conventions: a function takes the input file's bytes and returns the bytes it writes to
//! the output file. Row buffers come from the program's allocator (see [`crate::softpic`]).

use crate::effect::Fault;
use crate::softpic::{self, si_malloc, PicError, PicIn};
use si_file::format_g;
use si_picture::{ALPHA, BLUE, GREEN, MAX_ALLOC, RED};
use si_x87::{Round, F80};

/// The seven coefficients of the cubic the filter is built from
/// [soft 0x085dc7a0 mitchellInit]: the two-parameter family of piecewise cubics, here always
/// with both parameters 1/3 (as a float). They are stored as floats.
///
/// With `b` and `c` the parameters, each a sum in the register divided by 6:
/// `p0 = (6 − 2b)/6`, `p2 = (12b − 18 + 6c)/6`, `p3 = (12 − 9b − 6c)/6`,
/// `q0 = (8b + 24c)/6`, `q1 = (−12b − 48c)/6`, `q2 = (30c + 6b)/6`, `q3 = (−b − 6c)/6`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mitchell {
    pub p0: f32,
    pub p2: f32,
    pub p3: f32,
    pub q0: f32,
    pub q1: f32,
    pub q2: f32,
    pub q3: f32,
}

impl Mitchell {
    pub fn new(b: f32, c: f32) -> Mitchell {
        let k = |v: f64| F80::from_f64(v);
        let (b, c) = (F80::from_f32(b), F80::from_f32(c));
        let six = k(6.0);
        let c6 = c * six;
        Mitchell {
            p0: ((six - (b + b)) / six).to_f32(),
            p2: (((b * k(12.0) + k(-18.0)) + c6) / six).to_f32(),
            p3: (((k(12.0) - b * k(9.0)) - c6) / six).to_f32(),
            q0: ((b * k(8.0) + c * k(24.0)) / six).to_f32(),
            q3: ((-b - c6) / six).to_f32(),
            q2: ((c * k(30.0) + b * six) / six).to_f32(),
            q1: ((b * k(-12.0) - c * k(48.0)) / six).to_f32(),
        }
    }

    /// The program's filter: both parameters the float nearest 1/3.
    pub fn standard() -> Mitchell {
        Mitchell::new(f32::from_bits(0x3eaa_aaab), f32::from_bits(0x3eaa_aaab))
    }
}

/// Twice the cubic's value at `x / blur`, rounded to float [soft 0x085dc65c filterInteg].
///
/// Despite its name the routine integrates nothing. Called with `on` false it returns 0
/// before doing anything; the caller subtracts that from the call with `on` true, so a
/// weight is the filter's value at **one edge** of the source pixel, not an integral over it.
///
/// With `t = x / blur` (kept in the register):
/// `t < −2` → 0; `−2 ≤ t < −1` → `q0 − (q1 − (q2 − t·q3)·t)·t`;
/// `−1 ≤ t < 0` → `p0 + t²·(p2 − t·p3)`; `0 ≤ t < 1` → `p0 + t²·(t·p3 + p2)`;
/// `1 ≤ t < 2` → `q0 + ((t·q3 + q2)·t + q1)·t`; otherwise (and for a NaN) 0.
/// The value is doubled and stored to a float.
///
/// The original fills in the coefficients on its first call, from then on they are fixed:
/// they are [`Mitchell::standard`].
pub fn filter_integ(x: f32, on: bool, blur: f32, m: &Mitchell) -> f32 {
    if !on {
        return 0.0;
    }
    let t = F80::from_f32(x) / F80::from_f32(blur);
    let f = |v: f32| F80::from_f32(v);
    let v = if t.lt(F80::from_f64(-2.0)) {
        return 0.0;
    } else if t.lt(F80::from_f64(-1.0)) {
        f(m.q0) - (f(m.q1) - (f(m.q2) - t * f(m.q3)) * t) * t
    } else if F80::ZERO.gt(t) {
        (t * t) * (f(m.p2) - t * f(m.p3)) + f(m.p0)
    } else if F80::ONE.gt(t) {
        (t * t) * (t * f(m.p3) + f(m.p2)) + f(m.p0)
    } else if t.lt(F80::from_f64(2.0)) {
        ((t * f(m.q3) + f(m.q2)) * t + f(m.q1)) * t + f(m.q0)
    } else {
        return 0.0;
    };
    (v + v).to_f32()
}

/// The weights of one destination pixel: `n` source pixels from `first` on, their weights
/// (fixed point, 4096 = 1) and the weights' sum. The original's record is 16 bytes: n, sum,
/// a pointer to the first source value, a pointer to the weights.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub n: i32,
    pub total: i32,
    pub first: i32,
    pub weights: Vec<i16>,
}

fn floor_to_int(v: F80) -> i32 {
    // rounded down in the register, stored as a double, converted with truncation
    F80::from_f64(v.round_int(Round::Down).to_f64()).to_i32(Round::Zero)
}

/// The weight table for resampling `anx` source pixels to `bnx` [soft 0x085dc154 makeFilt],
/// and the largest `n` in it.
///
/// For destination pixel `x`, with `r = float(2·blur / min(anx, bnx))` and ε = 0.0001:
/// - reducing (`bnx < anx`): centre `c = float((x + 0.5)/bnx)`; the source pixels are
///   `floor((c − r)·anx + ε)` to `floor((c + r)·anx − ε)`; the weight of source pixel `s` is
///   the filter at `float(((s + 1)/anx − c)·bnx)`, the pixel's **far edge** in destination
///   pixels from the centre;
/// - otherwise: the destination pixel spans `x/bnx` (kept in the register) to
///   `float((x + 1)/bnx)`; the source pixels are `floor((lo − r)·anx + 0.5 + ε)` to
///   `floor((hi + r)·anx − 0.5 − ε)`; the weight of `s` is the filter at
///   `float((hi − float((s + 0.5)/anx))·anx)`.
/// The range is clipped to the source. A weight is trunc(value × 4096 + 0.5), kept in 16
/// bits.
pub fn make_filt(anx: i32, bnx: i32, blur: f32) -> Result<(Vec<Filter>, i32), Fault> {
    let m = Mitchell::standard();
    let (half, eps) = (F80::from_f64(0.5), F80::from_f64(0.0001));
    let (af, bf) = (F80::from_i32(anx), F80::from_i32(bnx));
    let (a32, b32) = (F80::from_f32(af.to_f32()), F80::from_f32(bf.to_f32()));
    let bl = F80::from_f32(blur);
    let weight = |v: f32| -> i16 { ((F80::from_f32(v) - F80::ZERO) * F80::from_f32(4096.0) + half).to_i32(Round::Zero) as i16 };
    let mut out = Vec::new();
    let mut most = 0;
    let mut held = 0usize;
    for x in 0..bnx.max(0) {
        let xf = F80::from_i32(x);
        let reduce = bnx < anx;
        let (mut lo, mut hi, centre, span);
        if reduce {
            let r = F80::from_f32(((bl + bl) / bf).to_f32());
            let c = F80::from_f32(((xf + half) / bf).to_f32());
            lo = floor_to_int((c - r) * af + eps);
            hi = floor_to_int((c + r) * af - eps);
            centre = c;
            span = (F80::ZERO, F80::ZERO);
        } else {
            let r = F80::from_f32(((bl + bl) / af).to_f32());
            let low = xf / bf;
            let high = F80::from_f32(((xf + F80::ONE) / bf).to_f32());
            lo = floor_to_int(af * (low - r) + half + eps);
            hi = floor_to_int((r + high) * af - half - eps);
            centre = F80::ZERO;
            span = (low, high);
        }
        if lo < 0 {
            lo = 0;
        }
        if hi >= anx {
            hi = anx - 1;
        }
        let n = hi.wrapping_sub(lo.wrapping_sub(1));
        held += n.max(0) as usize;
        if held > MAX_ALLOC / 2 {
            return Err(Fault::BadSize);
        }
        let mut f = Filter { n, total: 0, first: lo, weights: Vec::with_capacity(n.max(0) as usize) };
        for i in 0..n.max(0) {
            let at = if reduce {
                let s = F80::from_i32(lo) + F80::from_i32(i);
                // the near edge is worked out as well and handed to the call that returns 0
                (((s + F80::ONE) / a32 - centre) * b32).to_f32()
            } else {
                let c = F80::from_f32(((F80::from_i32(i.wrapping_add(lo)) + half) / af).to_f32());
                ((span.1 - c) * a32).to_f32()
            };
            let w = weight(filter_integ(at, true, blur, &m));
            f.weights.push(w);
            f.total = f.total.wrapping_add(w as i32);
        }
        if n > most {
            most = n;
        }
        out.push(f);
    }
    Ok((out, most))
}

/// One source row through the horizontal weights [soft 0x085dc5ac applyXFilt]: a pixel with
/// one source is copied; otherwise the weighted sum divided by the weights' sum (integer
/// division towards zero), kept in 16 bits.
pub fn apply_x_filt(out: &mut [i16], filt: &[Filter], src: &[i16]) -> Result<(), Fault> {
    for (o, f) in out.iter_mut().zip(filt) {
        if f.n < 0 {
            return Err(Fault::BadSize);
        }
        let s = src.get(f.first as usize..(f.first + f.n) as usize).ok_or(Fault::ShortPlane)?;
        if f.n == 1 {
            *o = s[0];
        } else {
            let sum = s.iter().zip(&f.weights).fold(0i32, |a, (&v, &w)| a.wrapping_add(w as i32 * v as i32));
            *o = sum.checked_div(f.total).ok_or(Fault::DivideError)? as i16;
        }
    }
    Ok(())
}

/// Adds a filtered row times a weight to the 32-bit sums [soft 0x085dbfa4 addRow].
pub fn add_row(acc: &mut [i32], row: &[i16], weight: i32) {
    for (a, &v) in acc.iter_mut().zip(row) {
        *a = a.wrapping_add((v as i32).wrapping_mul(weight));
    }
}

/// The sums divided by the weights' sum, kept in 16 bits [soft 0x085dc060 divRow].
pub fn div_row(acc: &[i32], out: &mut [i16], total: i32) -> Result<(), Fault> {
    for (o, &a) in out.iter_mut().zip(acc) {
        *o = a.checked_div(total).ok_or(Fault::DivideError)? as i16;
    }
    Ok(())
}

/// 16-bit values to bytes, negative ones 0 and those above 255 255 [soft 0x085dbf60 clampRow].
pub fn clamp_row(row: &[i16], out: &mut [u8]) {
    for (o, &v) in out.iter_mut().zip(row) {
        *o = v.clamp(0, 255) as u8;
    }
}

/// Why a file-to-file resizer gives up.
#[derive(Clone, Debug, PartialEq)]
pub enum ZoomError {
    /// The input is not a picture file the program opens. `SI_filterZoom` adds
    /// "cannot open picture file %s." to the opener's own message.
    Open(PicError),
    /// "Unexpected End Of File in input file!": the input ended before the rows did. The
    /// output file exists and holds the bytes given here (the header and the rows so far).
    EndOfFile(Vec<u8>),
    /// The input ended inside a block of rows: `SI_pictureReduce` returns 0 without a
    /// message and without closing either file. The bytes are the output so far.
    Silent(Vec<u8>),
    Fault(Fault),
}

impl From<Fault> for ZoomError {
    fn from(f: Fault) -> Self {
        ZoomError::Fault(f)
    }
}

/// The comment the filtered zoom puts in the header: with the output name when that is
/// shorter than 15 characters, without it otherwise; the three numbers as C's `%g`.
pub fn zoom_comment(out_name: &str, zx: f32, zy: f32, blur: f32) -> String {
    let nums = format!("h fact = {}, v fact = {}, blur = {}\n", format_g(zx as f64), format_g(zy as f64), format_g(blur as f64));
    if out_name.len() < 15 {
        format!("Picture {out_name} zoomed; {nums}")
    } else {
        format!("Zoomed picture; {nums}")
    }
}

/// Resizes a picture file by real factors through the cubic filter
/// [soft 0x085db350 SI_filterZoom], with its helpers [soft 0x085dc154 makeFilt],
/// [soft 0x085dc65c filterInteg], [soft 0x085dc7a0 mitchellInit],
/// [soft 0x085dc5ac applyXFilt], [soft 0x085dbfa4 addRow], [soft 0x085dc060 divRow] and
/// [soft 0x085dbf60 clampRow].
///
/// - New width = trunc(width × `zx` + 0.5), new height = trunc(height × `zy` + 0.5). A `blur`
///   whose size is below 1 becomes 1 (a blur of −1 or below is used as given).
/// - The output header: ratio 1.0, `fields` 3, the comment of [`zoom_comment`] with the blur
///   as adjusted. The output has alpha exactly when the input has **more than one channel
///   packet**: a file with all four channels in one packet loses its alpha, and one with
///   two packets and no alpha gets a plane made from a buffer nobody filled.
/// - For each output row, for each channel in turn, source rows are read until the last one
///   the row's weights need, each filtered horizontally into a ring of rows; then the ring's
///   last `n` rows are summed with the vertical weights, divided and clamped.
/// - **Only the red channel's turn reads the file.** Green, blue and alpha take the row
///   buffer as the red turn left it, so when an output row needs several new source rows
///   (any vertical reduction) those channels get the last of them several times over: red is
///   filtered vertically, the others are in effect sampled. Reproduced.
/// - A row that cannot be read leaves the buffers as they were; nothing is reported.
/// - Rows are written in the mixed coding, colours then alpha.
///
/// `out_name` is used for the comment only; a name starting with `-` (standard output) makes
/// the original write a header from pointers it never set, which is a [`Fault`] here.
pub fn filter_zoom(data: &[u8], out_name: &str, zx: f32, zy: f32, blur: f32) -> Result<Vec<u8>, ZoomError> {
    let mut pic = PicIn::open(data).map_err(ZoomError::Open)?;
    let (w, h) = (pic.width as i32, pic.height as i32);
    let half = F80::from_f64(0.5);
    let bnx = (F80::from_i32(w) * F80::from_f32(zx) + half).to_i32(Round::Zero);
    let bny = (half + F80::from_i32(h) * F80::from_f32(zy)).to_i32(Round::Zero);
    let blur = if F80::ONE.gt(F80::from_f32(blur).abs()) { 1.0 } else { blur };
    if out_name.starts_with('-') {
        return Err(Fault::Uninitialised.into());
    }
    if w < 0 || bnx < 0 || bnx > i16::MAX as i32 || bny > i16::MAX as i32 {
        // the original takes these as buffer sizes and as 16-bit header fields at once
        return Err(Fault::BadSize.into());
    }
    let (xfilt, _) = make_filt(w, bnx, blur)?;
    let (yfilt, ring) = make_filt(h, bny, blur)?;
    let channels = if pic.packets.len() > 1 { 4 } else { 3 };
    let (wu, bu) = (w as usize, bnx as usize);
    if (ring.max(0) as usize).saturating_mul(bu).saturating_mul(8) > MAX_ALLOC || bu.saturating_mul(bny.max(0) as usize).saturating_mul(4) > MAX_ALLOC {
        return Err(Fault::BadSize.into());
    }
    let mut rows: Vec<Vec<Vec<i16>>> = (0..channels).map(|_| (0..ring.max(0)).map(|_| vec![0i16; bu]).collect()).collect();
    let mut made: Vec<Vec<u8>> = (0..channels).map(|_| vec![0u8; bu]).collect();
    let mut out = Vec::new();
    let comment = zoom_comment(out_name, zx, zy, blur);
    softpic::write_header(&mut out, false, (bnx as i16, bny as i16, 1.0, 3), [true, true, true, channels == 4], Some(comment.as_bytes()));
    let mut bufs: [Option<Vec<u8>>; 4] = [si_malloc(w), si_malloc(w), si_malloc(w), si_malloc(w)];
    let mut abuf = vec![0i16; wu];
    let mut bbuf = vec![0i16; bu];
    let mut acc = vec![0i32; bu];
    let mut read = [0i32; 4];
    for yf in yfilt.iter().take(bny.max(0) as usize) {
        // the original gets the last row from the record's pointer, which for the vertical
        // table is twice the first row's number: halved in a float, then n − 1 added
        let last = (F80::from_i32(yf.first.wrapping_mul(2)) * F80::from_f32(0.5) + F80::from_i32(yf.n.wrapping_sub(1))).to_i32(Round::Zero);
        for ch in 0..channels {
            while read[ch] <= last {
                if ch == 0 {
                    pic.scanline(&mut bufs);
                }
                if w > 0 {
                    let b = bufs[ch].as_ref().ok_or(Fault::NullPlane)?;
                    for (a, &v) in abuf.iter_mut().zip(b) {
                        *a = v as i16;
                    }
                }
                read[ch] += 1;
                if ring < 1 {
                    return Err(Fault::BadSize.into());
                }
                rows[ch].rotate_left(1);
                apply_x_filt(rows[ch].last_mut().unwrap(), &xfilt, &abuf)?;
            }
            if yf.n == 1 {
                clamp_row(rows[ch].last().ok_or(Fault::BadSize)?, &mut made[ch]);
            } else {
                acc.fill(0);
                for i in 0..yf.n.max(0) {
                    add_row(&mut acc, &rows[ch][(ring - yf.n + i) as usize], yf.weights[i as usize] as i32);
                }
                div_row(&acc, &mut bbuf, yf.total)?;
                clamp_row(&bbuf, &mut made[ch]);
            }
        }
        si_picture::write_mixed(&mut out, &[&made[0], &made[1], &made[2]]);
        if channels == 4 {
            si_picture::write_mixed(&mut out, &[&made[3]]);
        }
    }
    Ok(out)
}

/// The row buffers the two whole-factor resizers make: for every channel some packet of the
/// input names, one output row and a block of input rows.
fn named(pic: &PicIn) -> [bool; 4] {
    [RED, GREEN, BLUE, ALPHA].map(|bit| pic.packets.iter().any(|p| p.channel & bit != 0))
}

/// Enlarges a picture file by whole factors, a row at a time
/// [soft 0x085daf2c SI_pictureEnlargeDisk]: each pixel `fx` times, each row written `fy` times.
///
/// The output header has width × `fx` and height × `fy` as 16-bit values, ratio 1.0, `fields`
/// 3 and the caller's comment. Before each input row the end of the input is tested: at the
/// end the routine shows "Unexpected End Of File in input file!" and stops with the output
/// as far as written ([`ZoomError::EndOfFile`]). The routine reads red, green and blue
/// through pointers it sets only for channels the file names: a file lacking one is a
/// [`Fault`]. No function of the binary calls this one.
pub fn enlarge_disk(data: &[u8], to_stdout: bool, fx: i32, fy: i32, comment: Option<&[u8]>) -> Result<Vec<u8>, ZoomError> {
    let mut pic = PicIn::open(data).map_err(ZoomError::Open)?;
    let has = named(&pic);
    let nw = (pic.width as i32).wrapping_mul(fx);
    if !(has[0] && has[1] && has[2]) {
        return Err(Fault::Uninitialised.into());
    }
    if nw as usize > MAX_ALLOC && nw > 0 {
        return Err(Fault::BadSize.into());
    }
    let mut made: [Option<Vec<u8>>; 4] = [0, 1, 2, 3].map(|c| if has[c] { si_malloc(nw) } else { None });
    let mut bufs = pic.row_buffers();
    let mut out = Vec::new();
    let present = [0, 1, 2, 3].map(|c| made[c].is_some());
    softpic::write_header(&mut out, to_stdout, (nw as i16, pic.height.wrapping_mul(fy as i16), 1.0, 3), present, comment);
    for _ in 0..pic.height.max(0) {
        if pic.at_end() {
            return Err(ZoomError::EndOfFile(out));
        }
        pic.scanline(&mut bufs);
        if pic.width > 0 && fx > 0 {
            for c in 0..4 {
                if c == 3 && made[3].is_none() {
                    continue;
                }
                let (src, dst) = (bufs[c].as_ref().ok_or(Fault::NullPlane)?, made[c].as_mut().ok_or(Fault::NullPlane)?);
                for (x, &v) in src.iter().enumerate() {
                    dst[x * fx as usize..(x + 1) * fx as usize].fill(v);
                }
            }
        }
        for _ in 0..fy.max(0) {
            softpic::write_row(&mut out, to_stdout, [0, 1, 2, 3].map(|c| made[c].as_deref()));
        }
    }
    Ok(out)
}

/// Reduces a picture file by whole factors, a block of rows at a time
/// [soft 0x085da780 SI_pictureReduce]: each new pixel is
/// trunc(sum of the `fx × fy` block × float(1 / (fx × fy)) + 0.5), at most 255.
///
/// - New width = width / `fx`, new height = height / `fy` (integer divisions); header ratio
///   1.0, `fields` 3, the caller's comment.
/// - Only packets in the **mixed** coding are read; a raw packet's rows are not read at all,
///   and its channels keep whatever the buffers held.
/// - Red, green and blue are summed through pointers that are set only for channels the file
///   names, and alpha is summed when a pointer **that is never initialised for a file
///   without alpha** is not null: a file lacking any of the four channels is a [`Fault`] here.
/// - The end of the input before a block: the message "Unexpected End Of File in input
///   file!" ([`ZoomError::EndOfFile`]); inside a block: 0 at once, nothing said and no file
///   closed ([`ZoomError::Silent`]).
/// No function of the binary calls this one.
pub fn reduce(data: &[u8], to_stdout: bool, fx: i32, fy: i32, comment: Option<&[u8]>) -> Result<Vec<u8>, ZoomError> {
    let mut pic = PicIn::open(data).map_err(ZoomError::Open)?;
    if fx == 0 || fy == 0 {
        return Err(Fault::DivideError.into());
    }
    let w = pic.width as i32;
    let (nw, nh) = (w / fx, pic.height as i32 / fy);
    let cells = fx.wrapping_mul(fy);
    let inv = (F80::ONE / F80::from_i32(cells)).to_f32();
    let has = named(&pic);
    let block_len = w.wrapping_mul(fy);
    if block_len > 0 && block_len as usize > MAX_ALLOC {
        return Err(Fault::BadSize.into());
    }
    let mut made: [Option<Vec<u8>>; 4] = [0, 1, 2, 3].map(|c| if has[c] { si_malloc(nw) } else { None });
    let mut block: [Option<Vec<u8>>; 4] = [0, 1, 2, 3].map(|c| if has[c] { si_malloc(block_len) } else { None });
    let mut out = Vec::new();
    softpic::write_header(&mut out, to_stdout, (nw as i16, nh as i16, 1.0, 3), [0, 1, 2, 3].map(|c| made[c].is_some()), comment);
    let half = F80::from_f64(0.5);
    for _ in 0..nh.max(0) {
        if pic.at_end() {
            return Err(ZoomError::EndOfFile(out));
        }
        for row in 0..fy.max(0) {
            for k in 0..pic.packets.len() {
                if pic.at_end() {
                    return Err(ZoomError::Silent(out));
                }
                if pic.packets[k].kind & 0xf == si_picture::TYPE_MIXED {
                    if block.iter().zip(has).any(|(b, h)| h && b.is_none()) || w < 0 {
                        return Err(Fault::NullPlane.into());
                    }
                    pic.packet_mixed(k, &mut block, (row * w) as usize);
                }
            }
        }
        if nw > 0 {
            if !has.iter().all(|&h| h) {
                return Err(Fault::Uninitialised.into());
            }
            for x in 0..nw as usize {
                for c in 0..4 {
                    let b = block[c].as_ref().ok_or(Fault::NullPlane)?;
                    let mut sum = 0i32;
                    for row in 0..fy.max(0) as usize {
                        for i in 0..fx.max(0) as usize {
                            sum += *b.get(x * fx as usize + row * w as usize + i).ok_or(Fault::ShortPlane)? as i32;
                        }
                    }
                    let v = (F80::from_i32(sum) * F80::from_f32(inv) + half).to_i32(Round::Zero);
                    made[c].as_mut().ok_or(Fault::NullPlane)?[x] = if v > 0xff { 0xff } else { v as u8 };
                }
            }
        }
        softpic::write_row(&mut out, to_stdout, [0, 1, 2, 3].map(|c| made[c].as_deref()));
    }
    Ok(out)
}

/// A leftover debugging aid [soft 0x085db2ec debug]: prints the first four bytes of an open
/// file to standard error as one big-endian number, `0x%x` and a new line, and leaves the
/// file positioned at its end. Returns the text. A file shorter than four bytes prints
/// whatever the variable held (0 here). No function of the binary calls it.
pub fn debug(file: &[u8]) -> String {
    let v = file.get(..4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    format!("0x{v:x}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mitchell_coefficients_for_one_third() {
        let m = Mitchell::standard();
        // (6 − 2/3) / 6 = 8/9, and the outer piece starts at (8/3 + 8) / 6 = 16/9
        assert!((m.p0 - 8.0 / 9.0).abs() < 1e-6);
        assert!((m.q0 - 16.0 / 9.0).abs() < 1e-6);
        assert!((m.p2 + 2.0).abs() < 1e-6);
        assert!((m.p3 - 7.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn filter_integ_is_twice_the_cubic_and_zero_when_switched_off() {
        let m = Mitchell::standard();
        assert_eq!(filter_integ(0.3, false, 1.0, &m), 0.0);
        assert!((filter_integ(0.0, true, 1.0, &m) - 16.0 / 9.0).abs() < 1e-6);
        assert_eq!(filter_integ(2.0, true, 1.0, &m), 0.0);
        assert_eq!(filter_integ(-2.5, true, 1.0, &m), 0.0);
        // a blur of 2 stretches the filter: the value at 2 is the value at 1 unblurred
        assert_eq!(filter_integ(2.0, true, 2.0, &m), filter_integ(1.0, true, 1.0, &m));
    }

    #[test]
    fn a_zoom_by_one_weighs_a_pixel_and_its_right_neighbour_alike() {
        let (table, most) = make_filt(8, 8, 1.0).unwrap();
        assert_eq!(most, 5);
        let f = &table[3];
        assert_eq!((f.first, f.n), (1, 5));
        // the filter is read at 2.5, 1.5, 0.5, −0.5 and −1.5 pixels
        assert_eq!(f.weights[0], 0);
        assert_eq!(f.weights[1], f.weights[4]);
        assert_eq!(f.weights[2], f.weights[3]);
        assert!(f.weights[1] < 0 && f.weights[2] > 2000);
        assert_eq!(f.total, f.weights.iter().map(|&w| w as i32).sum::<i32>());
    }

    #[test]
    fn a_flat_row_stays_flat() {
        let (table, _) = make_filt(6, 9, 1.0).unwrap();
        let mut out = [0i16; 9];
        apply_x_filt(&mut out, &table, &[200; 6]).unwrap();
        assert_eq!(out, [200; 9]);
        let mut bytes = [0u8; 3];
        clamp_row(&[-5, 100, 300], &mut bytes);
        assert_eq!(bytes, [0, 100, 255]);
    }

    #[test]
    fn the_comment_names_short_outputs_only() {
        assert_eq!(zoom_comment("out.pic", 2.0, 0.5, 1.0), "Picture out.pic zoomed; h fact = 2, v fact = 0.5, blur = 1\n");
        assert_eq!(zoom_comment("a_much_longer_name.pic", 2.0, 0.5, 1.5), "Zoomed picture; h fact = 2, v fact = 0.5, blur = 1.5\n");
    }

    #[test]
    fn debug_prints_the_first_word() {
        assert_eq!(debug(&[0x53, 0x80, 0xf6, 0x34, 9]), "0x5380f634\n");
    }

    #[test]
    fn whole_factor_resizers_on_a_made_up_file() {
        use si_picture::Picture;
        let p = Picture { width: 2, height: 2, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![1, 2, 3, 4]), Some(vec![5, 6, 7, 8]), Some(vec![9, 10, 11, 12]), Some(vec![255, 0, 255, 0])] };
        let file = p.write(None, false);
        let big = Picture::read(&enlarge_disk(&file, false, 2, 1, None).unwrap()).unwrap().picture;
        assert_eq!((big.width, big.height), (4, 2));
        assert_eq!(big.planes[0].as_deref(), Some(&[1u8, 1, 2, 2, 3, 3, 4, 4][..]));
        let small = Picture::read(&reduce(&file, false, 2, 2, None).unwrap()).unwrap().picture;
        assert_eq!((small.width, small.height), (1, 1));
        // trunc(10 × float(1/4) + 0.5) = 3, and the mean of 255, 0, 255, 0 rounds up to 128
        assert_eq!(small.planes[0].as_deref(), Some(&[3u8][..]));
        assert_eq!(small.planes[3].as_deref(), Some(&[128u8][..]));
        let zoomed = Picture::read(&filter_zoom(&file, "o.pic", 2.0, 2.0, 1.0).unwrap()).unwrap().picture;
        assert_eq!((zoomed.width, zoomed.height), (4, 4));
        assert!(zoomed.planes[3].is_some());
    }
}
