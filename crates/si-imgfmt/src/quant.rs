// SPDX-License-Identifier: MIT
//! Colour quantisation: the colour histogram of a packed picture, the two box-splitting
//! methods that choose a palette from it, and the box representatives (finding 0014,
//! "Colour quantisation").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `quant.c` (0x085af7a0–0x085b45c4), with one routine
//! of `dgt_picture.c` it cannot do without.
//!
//! The design is the well-known one (a hash table of colours, boxes of colours split until
//! there are as many as the palette has entries, one colour per box), extended to four
//! channels. What is specific to this program, and reproduced: the hash and its chain order
//! (which fix the order of the histogram and so every later tie), the depth reduction when a
//! picture has more than 65,536 colours, the statistics kept in doubles with their rounding
//! points, and the slips noted at each function.
//!
//! The original keeps "alpha counts" in a global (`si_alphaOn`); here it is the `alpha`
//! argument. Progress reports and the test for an interrupt key that the original makes
//! between rows are left out (they are its `short` argument being non-zero).

use crate::effect::Fault;
use si_picture::PictureRgba;
use si_x87::{Round, F80};

/// Buckets of the colour hash table.
pub const HASH_SIZE: usize = 20023;
/// The most colours a histogram holds; one more and every channel loses a bit of depth.
pub const MAX_COLORS: i32 = 0x10000;

/// One colour and how often it occurs: the original's 12-byte histogram entry (also the
/// layout of a palette entry while quantising, where the third word's low byte holds flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistEntry {
    /// Red, green, blue, alpha, each reduced to the bits asked for.
    pub color: [u8; 4],
    pub count: i32,
    pub extra: i32,
}

/// A table that maps a value of `from` bits to one of `to` bits
/// [soft 0x085b9c10 SI_dgColorEquivTable] (`dgt_picture.c`): entry `i` is
/// trunc(i × (2^to − 1) / (2^from − 1) + 0.49999), the quotient kept in the register.
/// Note the constant: not one half.
pub fn color_equiv_table(from: i32, to: i32) -> Result<Vec<u8>, Fault> {
    if (from & 31) > 16 {
        return Err(Fault::BadSize);
    }
    let n = 1i32 << (from & 31);
    let k = F80::from_i32((1i32 << (to & 31)).wrapping_sub(1)) / F80::from_i32(n - 1);
    Ok((0..n).map(|i| (F80::from_i32(i) * k + F80::from_f64(0.49999)).to_i32(Round::Zero) as u8).collect())
}

/// Where a colour goes in the table [soft 0x085b251c addColorHashTable]
/// [soft 0x085b25b8 checkColorHashTable]: (33023 r + 30013 g + 27011 b + 24007 a) mod 20023.
/// Alpha is part of the hash whether or not alpha counts.
pub fn hash(c: [u8; 4]) -> usize {
    ((c[0] as u32 * 0x80ff + c[1] as u32 * 0x753d + c[2] as u32 * 0x6983 + c[3] as u32 * 0x5dc7) % HASH_SIZE as u32) as usize
}

/// The colour hash table [soft 0x085b1440 allocColorHashTable]
/// [soft 0x085b145c freeColorHashTable]: 20,023 chains. A new colour goes to the **head** of
/// its chain; here each bucket is kept in order of arrival and read backwards.
#[derive(Clone, Debug)]
pub struct ColorHash {
    buckets: Vec<Vec<HistEntry>>,
}

impl Default for ColorHash {
    fn default() -> Self {
        ColorHash { buckets: vec![Vec::new(); HASH_SIZE] }
    }
}

impl ColorHash {
    /// Two colours are the same when red, green and blue agree, and alpha too if it counts.
    /// Since alpha is hashed all the same, two colours differing only in alpha are found
    /// equal only when they happen to share a bucket.
    fn find(&mut self, c: [u8; 4], alpha: bool) -> Option<&mut HistEntry> {
        self.buckets[hash(c)].iter_mut().rev().find(|e| e.color[..3] == c[..3] && (!alpha || e.color[3] == c[3]))
    }

    /// Files a colour with a value, without looking for it first
    /// [soft 0x085b251c addColorHashTable].
    pub fn add(&mut self, c: [u8; 4], value: i32) {
        self.buckets[hash(c)].push(HistEntry { color: c, count: value, extra: 0 });
    }

    /// The value filed for a colour, or −1 [soft 0x085b25b8 checkColorHashTable].
    pub fn check(&mut self, c: [u8; 4], alpha: bool) -> i32 {
        self.find(c, alpha).map_or(-1, |e| e.count)
    }

    /// The table's entries as a list [soft 0x085b13c8 colorTable2ColorHistVectorList]: bucket
    /// by bucket, each chain from its head. The original's list always has room for 65,536
    /// entries, the unused ones zero; this one has the entries only.
    pub fn to_list(&self) -> Vec<HistEntry> {
        self.buckets.iter().flat_map(|b| b.iter().rev().copied()).collect()
    }

    /// Numbers the table's entries in the order of [`ColorHash::to_list`], storing each
    /// number as the entry's value, and returns their colours in that order (the walk
    /// `si_paletteFilterRedundancy` makes over the chains).
    pub fn number(&mut self) -> Vec<[u8; 4]> {
        let mut out = Vec::new();
        for b in self.buckets.iter_mut() {
            for e in b.iter_mut().rev() {
                e.count = out.len() as i32;
                out.push(e.color);
            }
        }
        out
    }

    /// Halves the depth of every channel that has more than one bit and files every colour
    /// again, adding up the counts of those that fall together
    /// [soft 0x085b0ca0 reduceColorHashTable]. `bits` is updated; the result is the new
    /// table and its number of colours. Colours are taken bucket by bucket, each chain from
    /// its head.
    pub fn reduce(self, bits: &mut [i32; 4], alpha: bool) -> Result<(ColorHash, i32), Fault> {
        let mut luts: Vec<Vec<u8>> = Vec::with_capacity(4);
        for b in bits.iter_mut() {
            if *b < 2 {
                luts.push(vec![0, 1]);
            } else {
                luts.push(color_equiv_table(*b, *b - 1)?);
                *b -= 1;
            }
        }
        let mut out = ColorHash::default();
        let mut count = 0;
        for e in self.to_list() {
            let mut c = [0u8; 4];
            for k in 0..4 {
                c[k] = *luts[k].get(e.color[k] as usize).ok_or(Fault::ShortPlane)?;
            }
            match out.find(c, alpha) {
                Some(found) => found.count = found.count.wrapping_add(e.count),
                None => {
                    out.add(c, e.count);
                    count += 1;
                }
            }
        }
        Ok((out, count))
    }
}

/// Counts the colours of a packed picture into a table
/// [soft 0x085b0ff4 makeColorHashTable].
///
/// Every channel of every pixel is first reduced from 8 bits to `bits` of that channel
/// ([`color_equiv_table`]). With `table` given, the picture's colours are added to it and
/// `count` goes on from its value (how a palette for several pictures is made:
/// [soft 0x085b0c50 addToColorTable] is this call with the table kept between pictures).
/// When the count passes 65,536 the table is reduced ([`ColorHash::reduce`]) and the
/// remaining pixels are read at the new depths.
pub fn make_color_hash_table(p: &PictureRgba, bits: &mut [i32; 4], count: &mut i32, table: Option<ColorHash>, alpha: bool) -> Result<ColorHash, Fault> {
    let mut table = match table {
        Some(t) => t,
        None => {
            *count = 0;
            ColorHash::default()
        }
    };
    let lut = |bits: &[i32; 4]| -> Result<Vec<Vec<u8>>, Fault> { bits.iter().map(|&b| color_equiv_table(8, b)).collect() };
    let mut luts = lut(bits)?;
    let (w, h) = (p.width.max(0) as usize, p.height.max(0) as usize);
    if p.data.len() < 4 * w * h {
        return Err(Fault::ShortPlane);
    }
    for px in p.data[..4 * w * h].chunks_exact(4) {
        let c = [luts[0][px[0] as usize], luts[1][px[1] as usize], luts[2][px[2] as usize], luts[3][px[3] as usize]];
        if let Some(e) = table.find(c, alpha) {
            e.count = e.count.wrapping_add(1);
            continue;
        }
        table.add(c, 1);
        *count += 1;
        if *count > MAX_COLORS {
            let (t, n) = table.reduce(bits, alpha)?;
            table = t;
            *count = n;
            luts = lut(bits)?;
        }
    }
    Ok(table)
}

/// The histogram of a picture as a list [soft 0x085b0c00 makeColorHistVectorList]: the
/// colours and their number.
pub fn make_color_hist_vector_list(p: &PictureRgba, bits: &mut [i32; 4], alpha: bool) -> Result<(Vec<HistEntry>, i32), Fault> {
    let mut count = 0;
    let table = make_color_hash_table(p, bits, &mut count, None, alpha)?;
    Ok((table.to_list(), count))
}

/// A box of colours: a stretch of the histogram list (the original's 52-byte record).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorBox {
    /// First entry in the list.
    pub index: i32,
    /// Number of entries.
    pub colors: i32,
    /// Pixels these colours stand for.
    pub sum: u32,
    /// Mean of each channel, weighted by the counts (variance method only).
    pub mean: [f64; 4],
    /// Weighted variance, summed over the channels, divided by `total`.
    pub variance: f32,
    /// Pixels of the whole picture.
    pub total: u32,
}

fn stretch<'a>(b: &ColorBox, hist: &'a [HistEntry]) -> Result<&'a [HistEntry], Fault> {
    if b.colors <= 0 {
        return Ok(&[]);
    }
    usize::try_from(b.index).ok().and_then(|i| hist.get(i..i + b.colors as usize)).ok_or(Fault::ShortPlane)
}

fn stretch_mut<'a>(b: &ColorBox, hist: &'a mut [HistEntry]) -> Result<&'a mut [HistEntry], Fault> {
    if b.colors <= 0 {
        return Ok(&mut []);
    }
    usize::try_from(b.index).ok().and_then(|i| hist.get_mut(i..i + b.colors as usize)).ok_or(Fault::ShortPlane)
}

/// The mean and the variance of a box [soft 0x085b14c4 boxStats].
///
/// Per channel (three, or four when alpha counts) two sums are kept in doubles, each
/// addition rounded: Σ value × count and Σ value × (value × count). The mean is the first
/// sum divided by the box's pixels, used **unrounded** for the variance and stored rounded;
/// the variance collects `sum of squares − mean² × pixels` channel by channel in a float and
/// is divided by the picture's pixels at the end. A box without colours gets variance 0 and
/// nothing else.
pub fn box_stats(b: &mut ColorBox, hist: &[HistEntry], alpha: bool) -> Result<(), Fault> {
    b.variance = 0.0;
    if b.colors == 0 {
        return Ok(());
    }
    let channels = if alpha { 4 } else { 3 };
    let mut squares = [0f64; 4];
    b.mean[..channels].fill(0.0);
    for e in stretch(b, hist)? {
        for c in 0..channels {
            let v = e.color[c] as i32;
            let weighted = v.wrapping_mul(e.count);
            b.mean[c] = (F80::from_f64(b.mean[c]) + F80::from_i32(weighted)).to_f64();
            squares[c] = (F80::from_i32(v) * F80::from_i32(weighted) + F80::from_f64(squares[c])).to_f64();
        }
    }
    let pixels = F80::from_i64(b.sum as i64);
    for c in 0..channels {
        let mean = F80::from_f64(b.mean[c]) / pixels;
        b.mean[c] = mean.to_f64();
        b.variance = (F80::from_f32(b.variance) + (F80::from_f64(squares[c]) - (mean * mean) * pixels)).to_f32();
    }
    b.variance = (F80::from_f32(b.variance) / F80::from_i64(b.total as i64)).to_f32();
    Ok(())
}

/// The box with the largest variance above −1, the first of equals
/// [soft 0x085b18f8 maxVariance].
pub fn max_variance(boxes: &[ColorBox]) -> Option<usize> {
    let mut best = F80::from_f32(-1.0);
    let mut at = None;
    for (i, b) in boxes.iter().enumerate() {
        if best.lt(F80::from_f32(b.variance)) {
            best = F80::from_f32(b.variance);
            at = Some(i);
        }
    }
    at
}

/// The box with the most colours, the first of equals [soft 0x085b1940 maxNumColors].
pub fn max_num_colors(boxes: &[ColorBox]) -> Option<usize> {
    let mut best = -1;
    let mut at = None;
    for (i, b) in boxes.iter().enumerate() {
        if best < b.colors {
            best = b.colors;
            at = Some(i);
        }
    }
    at
}

/// Sorts a stretch of the list by one channel, as `qsort` with
/// [soft 0x085b24b0 redCompare], [soft 0x085b24c8 greenCompare],
/// [soft 0x085b24e4 blueCompare] or [soft 0x085b2500 alphaCompare] does. The C library's
/// `qsort` is a merge sort and keeps equal values in their order, which decides every tie
/// from here on; so does this.
fn sort_by_channel(list: &mut [HistEntry], channel: usize) {
    list.sort_by_key(|e| e.color[channel]);
}

/// The best place to cut a box whose colours are sorted by `channel`
/// [soft 0x085b1ccc varFindCutPoint]: the two halves with their statistics, or nothing for a
/// box of fewer than two colours.
///
/// Walking the colours with `n` the pixels so far and `m` the running sum of
/// value × count / pixels of the box (kept in the register), the cut after a colour scores
/// `float(n / (pixels − n) × (mean − m)²)`; the largest score above −1 wins, the first of
/// equals. The last colour is never a cut. If no colour scores (the first already holds all
/// the pixels) the original goes on with variables it never set: a [`Fault`] here.
pub fn var_find_cut_point(b: &ColorBox, channel: usize, hist: &[HistEntry], alpha: bool) -> Result<Option<(ColorBox, ColorBox)>, Fault> {
    if b.colors < 2 {
        return Ok(None);
    }
    let pixels = F80::from_i64(b.sum as i64);
    let mean = F80::from_f64(b.mean[channel]);
    let mut running = F80::ZERO;
    let mut seen = 0u32;
    let mut best = -1.0f32;
    let mut cut = None;
    for (i, e) in stretch(b, hist)?.iter().enumerate() {
        seen = seen.wrapping_add(e.count as u32);
        if seen == b.sum {
            break;
        }
        running = running + F80::from_i32(e.count.wrapping_mul(e.color[channel] as i32)) / pixels;
        let off = mean - running;
        let score = (F80::from_i64(seen as i64) / F80::from_i64(b.sum.wrapping_sub(seen) as i64) * off * off).to_f32();
        if F80::from_f32(score).gt(F80::from_f32(best)) {
            best = score;
            cut = Some((i as i32 + 1, seen));
        }
    }
    let (colors, pixels_low) = cut.ok_or(Fault::Uninitialised)?;
    let (mut low, mut high) = (*b, *b);
    low.sum = pixels_low;
    high.sum = high.sum.wrapping_sub(pixels_low);
    low.colors = colors;
    high.colors -= colors;
    high.index += colors;
    box_stats(&mut low, hist, alpha)?;
    box_stats(&mut high, hist, alpha)?;
    Ok(Some((low, high)))
}

/// Splits a box in two by the variance rule [soft 0x085b1980 varCutBox]. Returns false,
/// with nothing changed but the order of the box's colours, when the box cannot be split
/// (its variance is exactly 0, or it holds no pixels).
///
/// For each channel in turn (red, green, blue, and alpha when it counts) the box's colours
/// are sorted by that channel and the best cut found; the channel's score is the sum of the
/// two halves' variances as a float, or the largest float if there is no cut. The channel
/// with the smallest score is taken: red unless green is smaller or blue or alpha are
/// smaller; else green if it is not above any other; else alpha if it is below blue; else
/// blue. For red and green the colours are sorted and cut again (the list was left sorted
/// by the last channel tried); blue is sorted again only when alpha counts.
pub fn var_cut_box(b: &mut ColorBox, hist: &mut [HistEntry], alpha: bool) -> Result<Option<ColorBox>, Fault> {
    let channels = if alpha { 4 } else { 3 };
    if F80::from_f32(b.variance).eq(F80::ZERO) || b.sum == 0 {
        return Ok(None);
    }
    let mut score = [0f32; 4];
    let mut halves: [Option<(ColorBox, ColorBox)>; 4] = [None; 4];
    for c in 0..channels {
        sort_by_channel(stretch_mut(b, hist)?, c);
        halves[c] = var_find_cut_point(b, c, hist, alpha)?;
        score[c] = match &halves[c] {
            Some((low, high)) => (F80::from_f32(low.variance) + F80::from_f32(high.variance)).to_f32(),
            None => f32::MAX,
        };
    }
    let s = score.map(F80::from_f32);
    let above = |a: F80, c: F80| a.gt(c) || a.unordered(c);
    let below = |a: F80, c: F80| a.lt(c) || a.unordered(c);
    let chosen = if !(below(s[1], s[0]) || above(s[0], s[2]) || (alpha && above(s[0], s[3]))) {
        0
    } else if !(above(s[1], s[0]) || above(s[1], s[2]) || (alpha && above(s[1], s[3]))) {
        1
    } else if alpha && below(s[3], s[2]) {
        3
    } else {
        2
    };
    if chosen < 2 || (chosen == 2 && alpha) {
        sort_by_channel(stretch_mut(b, hist)?, chosen);
        halves[chosen] = var_find_cut_point(b, chosen, hist, alpha)?;
    }
    // a box of one colour reaches this point with no halves at all: the original copies two
    // records it only cleared
    let (low, high) = halves[chosen].unwrap_or_default();
    *b = low;
    Ok(Some(high))
}

/// Splits a box in two by the median rule [soft 0x085b1e58 medianCutBox]: the colours are
/// sorted by the channel with the widest range (red if it is not narrower than any other,
/// else green if it is not narrower than blue or alpha, else alpha if wider than blue, else
/// blue; alpha has no range when it does not count) and cut where the pixels counted reach
/// half of the box's.
///
/// The walk: colours are added while the count is below half and more than one colour is
/// left over; if the walk stops because half was reached, the last colour is given back when
/// that brings the count nearer to half. A box of two colours is cut between them; the walk
/// that runs out of colours keeps all but the last. Means and variances are not touched.
pub fn median_cut_box(b: &mut ColorBox, hist: &mut [HistEntry], alpha: bool) -> Result<ColorBox, Fault> {
    let (index, n, sum) = (b.index, b.colors, b.sum);
    let list = stretch_mut(b, hist)?;
    let first = list.first().map_or([0; 4], |e| e.color);
    let (mut lo, mut hi) = (first, first);
    if !alpha {
        lo[3] = 0xff;
        hi[3] = 0xff;
    }
    for e in list.iter().skip(1) {
        for c in 0..if alpha { 4 } else { 3 } {
            lo[c] = lo[c].min(e.color[c]);
            hi[c] = hi[c].max(e.color[c]);
        }
    }
    let range = [0, 1, 2, 3].map(|c| hi[c] as i32 - lo[c] as i32);
    let channel = if !(range[0] < range[1] || range[0] < range[2] || range[0] < range[3]) {
        0
    } else if !(range[2] > range[1] || range[1] < range[3]) {
        1
    } else if range[2] < range[3] {
        3
    } else {
        2
    };
    sort_by_channel(list, channel);
    let half = (sum as i32 / 2) as u32;
    let count = |i: usize| list.get(i).map(|e| e.count as u32).ok_or(Fault::ShortPlane);
    let mut seen = count(0)?;
    let mut lower = 1i32;
    if 1 < n - 1 {
        if seen < half {
            let mut reached = false;
            loop {
                seen = seen.wrapping_add(count(lower as usize)?);
                lower += 1;
                if n - 1 <= lower {
                    break;
                }
                if seen >= half {
                    reached = true;
                    break;
                }
            }
            if reached {
                let last = count(lower as usize - 1)?;
                if half.wrapping_sub(seen.wrapping_sub(last)) < seen.wrapping_sub(half) {
                    seen = seen.wrapping_sub(last);
                    lower -= 1;
                }
            }
        } else if half.wrapping_sub(seen.wrapping_sub(count(0)?)) < seen.wrapping_sub(half) {
            seen = 0;
            lower = 0;
        }
    }
    b.colors = lower;
    b.sum = seen;
    Ok(ColorBox { index: index + lower, colors: n - lower, sum: sum.wrapping_sub(seen), ..*b })
}

/// What [`compute_box_color_representatives`] prints for a method it does not know.
pub const BAD_METHOD: &str = "Error, invalid box color assignation method!\n";

/// One colour per box [soft 0x085b20e0 computeBoxColorRepresentatives], by `method`:
/// - 1: the middle of the box, (least + greatest) / 2 per channel. **For alpha the range is
///   taken from the first colour's alpha and the other colours' blue**: a slip, reproduced.
/// - 2: the mean of the box's colours, each counted once (integer division).
/// - 3: the mean weighted by the counts (unsigned sums, limited to 255).
/// - anything else: [`BAD_METHOD`] on standard error (returned as `Err`), nothing written.
///
/// Alpha is 255 when it does not count. A box without colours makes methods 2 and 3 divide
/// by zero.
pub fn compute_box_color_representatives(boxes: &[ColorBox], map: &mut [HistEntry], method: i32, hist: &[HistEntry], alpha: bool) -> Result<Result<(), &'static str>, Fault> {
    if !(1..=3).contains(&method) {
        return Ok(Err(BAD_METHOD));
    }
    for (b, out) in boxes.iter().zip(map.iter_mut()) {
        let list = stretch(b, hist)?;
        let color = match method {
            1 => {
                // read even from an empty box
                let first = usize::try_from(b.index).ok().and_then(|i| hist.get(i)).ok_or(Fault::ShortPlane)?.color;
                let (mut lo, mut hi) = (first, first);
                for e in list.iter().skip(1) {
                    for c in 0..3 {
                        lo[c] = lo[c].min(e.color[c]);
                        hi[c] = hi[c].max(e.color[c]);
                    }
                    lo[3] = lo[3].min(e.color[2]);
                    hi[3] = hi[3].max(e.color[2]);
                }
                [0, 1, 2, 3].map(|c| ((lo[c] as u32 + hi[c] as u32) / 2) as u8)
            }
            2 => {
                let mut sum = [0i32; 4];
                for e in list {
                    for c in 0..4 {
                        sum[c] += e.color[c] as i32;
                    }
                }
                if b.colors == 0 {
                    return Err(Fault::DivideError);
                }
                sum.map(|s| (s / b.colors) as u8)
            }
            _ => {
                let (mut sum, mut pixels) = ([0u32; 4], 0u32);
                for e in list {
                    for c in 0..4 {
                        sum[c] = sum[c].wrapping_add((e.color[c] as u32).wrapping_mul(e.count as u32));
                    }
                    pixels = pixels.wrapping_add(e.count as u32);
                }
                if pixels == 0 {
                    return Err(Fault::DivideError);
                }
                sum.map(|s| (s / pixels).min(0xff) as u8)
            }
        };
        out.color = [color[0], color[1], color[2], if alpha { color[3] } else { 0xff }];
    }
    Ok(Ok(()))
}

/// Why [`make_color_map`] returns no palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    /// No entries were asked for: "Error, unable to allocate boxVector structure\n" on
    /// standard error.
    NoBoxes,
    /// A cutting method other than 1 and 2 with more than one entry asked for: nothing is
    /// said.
    BadCut,
    Fault(Fault),
}

impl From<Fault> for MapError {
    fn from(f: Fault) -> Self {
        MapError::Fault(f)
    }
}

/// Chooses `wanted` colours for a histogram [soft 0x085b1684 makeColorMap].
///
/// `colors` and `pixels` describe the histogram list (`hist`, whose order is changed);
/// `cut` is 1 for the median rule ([`median_cut_box`], always splitting the box with the
/// most colours) or 2 for the variance rule ([`var_cut_box`], always splitting the box with
/// the largest variance); `method` chooses the representatives
/// ([`compute_box_color_representatives`]). Splitting stops when there are `wanted` boxes,
/// when the box chosen has fewer than two colours, or when the variance rule cannot split.
///
/// The palette has `wanted` entries; those beyond the boxes made stay black with alpha 255.
/// The second value is what the routine printed on standard error.
pub fn make_color_map(hist: &mut [HistEntry], colors: i32, pixels: u32, wanted: i32, cut: i32, method: i32, alpha: bool) -> Result<(Vec<HistEntry>, &'static str), MapError> {
    if wanted <= 0 {
        return Err(MapError::NoBoxes);
    }
    if wanted as usize > 1 << 20 {
        return Err(Fault::BadSize.into());
    }
    let mut map = vec![HistEntry { color: [0, 0, 0, 0xff], count: 0, extra: 0 }; wanted as usize];
    let mut boxes = vec![ColorBox { index: 0, colors, sum: pixels, ..ColorBox::default() }];
    if cut == 2 {
        boxes[0].total = pixels;
        box_stats(&mut boxes[0], hist, alpha)?;
    }
    while (boxes.len() as i32) < wanted {
        let pick = match cut {
            2 => max_variance(&boxes),
            1 => max_num_colors(&boxes),
            _ => return Err(MapError::BadCut),
        };
        let Some(pick) = pick else { break };
        if boxes[pick].colors < 2 {
            break;
        }
        let new = if cut == 2 {
            match var_cut_box(&mut boxes[pick], hist, alpha)? {
                Some(b) => b,
                None => break,
            }
        } else {
            median_cut_box(&mut boxes[pick], hist, alpha)?
        };
        boxes.push(new);
    }
    let said = match compute_box_color_representatives(&boxes, &mut map, method, hist, alpha)? {
        Ok(()) => "",
        Err(text) => text,
    };
    Ok((map, said))
}

/// The squared distance of two colours over all four channels
/// [soft 0x085b39e8 computeRGBDistance].
pub fn compute_rgb_distance(a: [i32; 4], b: [i32; 4]) -> i32 {
    (0..4).fold(0i32, |s, c| s.wrapping_add((a[c].wrapping_sub(b[c])).wrapping_mul(a[c].wrapping_sub(b[c]))))
}

/// What the mapping needs to know about the palette: how many entries, and the depth of
/// each channel of their colours (the original's `QuantOptions`, kept in globals between
/// the palette's making and its use).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantOptions {
    pub colors: i32,
    pub bits: [i32; 4],
}

/// Flag in the low byte of a palette entry's third word: the entry may be used. When any
/// entry carries it, the mapping uses the flagged entries only.
pub const ENTRY_USABLE: i32 = 0x10;

/// Maps every pixel of a packed picture to the nearest palette entry
/// [soft 0x085aff68 reduceColors], with error diffusion when `dither` is 1: one 16-bit
/// index per pixel, first row first as the picture has them.
///
/// - Palette colours are brought back to 8 bits per channel ([`color_equiv_table`] from the
///   channel's depth to 8) and compared with the pixel by the sum of squared differences
///   over four channels; the first of equally near entries wins. When alpha does not count,
///   the alpha difference is 0. If some entries carry [`ENTRY_USABLE`], only those are
///   candidates.
/// - Every colour met is remembered in a hash table with its entry ([`ColorHash::add`],
///   [`ColorHash::check`]), so equal pixels map alike. With dithering the colour looked up
///   is the pixel **after** the error was added.
/// - Dithering (integer, errors scaled by 1024): a pixel gets the error collected for it,
///   divided by 1024 towards zero, and is limited to 0…255; its difference from the entry
///   chosen goes 7/16 to the next pixel in the direction of travel, 3/16, 5/16 and 1/16 to
///   the row below (behind, under, ahead), each share divided towards zero. Rows alternate
///   direction, the first from the left. Alpha takes part only when it counts.
/// - A picture with an empty palette maps everything to −1 (0xffff); with dithering the
///   original then reads in front of the palette, which is a [`Fault`] here. `dither` values
///   other than 0 and 1 make it test a variable it never set.
pub fn reduce_colors(p: &PictureRgba, opts: &QuantOptions, map: &[HistEntry], dither: i16, alpha: bool) -> Result<Vec<u16>, Fault> {
    if dither != 0 && dither != 1 {
        return Err(Fault::Uninitialised);
    }
    let (w, h) = (p.width as i32, p.height as i32);
    let n = opts.colors.max(0) as usize;
    let map = map.get(..n).ok_or(Fault::ShortPlane)?;
    let restricted = map.iter().any(|e| e.extra & ENTRY_USABLE != 0);
    let mut seen = ColorHash::default();
    let luts: Vec<Vec<u8>> = opts.bits.iter().map(|&b| color_equiv_table(b, 8)).collect::<Result<_, _>>()?;
    let wide = |e: &HistEntry, c: usize| luts[c].get(e.color[c] as usize).copied().ok_or(Fault::ShortPlane).map(|v| v as i32);
    if h <= 0 {
        return Ok(Vec::new());
    }
    if w <= 0 {
        // the original's column loop runs until its counter comes round to the row's end
        return Err(Fault::BadSize);
    }
    let (wu, hu) = (w as usize, h as usize);
    if p.data.len() < 4 * wu * hu {
        return Err(Fault::ShortPlane);
    }
    let mut out = vec![0u16; wu * hu];
    let fs = dither == 1;
    let mut this = [vec![0i32; wu + 2], vec![0i32; wu + 2], vec![0i32; wu + 2], vec![0i32; wu + 2]];
    let mut next = this.clone();
    let mut forward = true;
    for y in 0..hu {
        if fs {
            next.iter_mut().for_each(|r| r.fill(0));
        }
        for step in 0..wu {
            let col = if !fs || forward { step } else { wu - 1 - step };
            let at = y * wu + col;
            let mut px = [p.data[4 * at], p.data[4 * at + 1], p.data[4 * at + 2], p.data[4 * at + 3]];
            let mut exact = [0i32; 4];
            if fs {
                for c in 0..4 {
                    exact[c] = (this[c][col + 1] / 1024 + px[c] as i32).clamp(0, 255);
                    px[c] = exact[c] as u8;
                }
            }
            let mut index = seen.check(px, alpha);
            if index == -1 {
                let mut best = u32::MAX;
                for (i, e) in map.iter().enumerate() {
                    if restricted && e.extra & ENTRY_USABLE == 0 {
                        continue;
                    }
                    let a = if alpha { wide(e, 3)? } else { px[3] as i32 };
                    let d = [px[0] as i32 - wide(e, 0)?, px[1] as i32 - wide(e, 1)?, px[2] as i32 - wide(e, 2)?, px[3] as i32 - a];
                    let dist = (d[3] * d[3] + d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as u32;
                    if dist < best {
                        best = dist;
                        index = i as i32;
                    }
                }
                seen.add(px, index);
            }
            if fs {
                let e = usize::try_from(index).ok().and_then(|i| map.get(i)).ok_or(Fault::ShortPlane)?;
                for c in 0..if alpha { 4 } else { 3 } {
                    let err = exact[c] - wide(e, c)?;
                    let share = |k: i32| err.wrapping_mul(k * 1024) / 16;
                    let (ahead, behind) = if forward { (col + 2, col) } else { (col, col + 2) };
                    this[c][ahead] = this[c][ahead].wrapping_add(share(7));
                    next[c][behind] = next[c][behind].wrapping_add(share(3));
                    next[c][col + 1] = next[c][col + 1].wrapping_add(share(5));
                    next[c][ahead] = next[c][ahead].wrapping_add(share(1));
                }
            }
            out[at] = index as u16;
        }
        if fs {
            std::mem::swap(&mut this, &mut next);
            forward = !forward;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(c: [u8; 4], count: i32) -> HistEntry {
        HistEntry { color: c, count, extra: 0 }
    }

    #[test]
    fn depth_tables_use_a_constant_just_below_one_half() {
        assert_eq!(color_equiv_table(2, 8).unwrap(), vec![0, 85, 170, 255]);
        let t = color_equiv_table(8, 1).unwrap();
        assert_eq!((t[127], t[128], t[255]), (0, 1, 1)); // 127 / 255 + 0.49999 is still below 1
        assert_eq!(color_equiv_table(8, 8).unwrap()[200], 200);
    }

    #[test]
    fn the_histogram_follows_the_buckets_and_reads_each_chain_from_its_head() {
        let px = [[1u8, 2, 3, 255], [9, 9, 9, 255], [1, 2, 3, 255], [1, 2, 3, 0]];
        let p = PictureRgba { width: 4, height: 1, ratio: 1.0, fields: 3, pad: 0, data: px.concat() };
        let (list, n) = make_color_hist_vector_list(&p, &mut [8; 4], true).unwrap();
        assert_eq!(n, 3);
        let mut want = vec![entry([1, 2, 3, 255], 2), entry([9, 9, 9, 255], 1), entry([1, 2, 3, 0], 1)];
        want.sort_by_key(|e| hash(e.color));
        assert_eq!(list, want);
        // without alpha the two would be one colour, but they hash apart and stay two
        let (list, n) = make_color_hist_vector_list(&p, &mut [8; 4], false).unwrap();
        assert_eq!((n, list.len()), (3, 3));
    }

    #[test]
    fn the_median_rule_cuts_the_widest_channel_at_half_the_pixels() {
        let mut hist = vec![entry([200, 0, 0, 255], 5), entry([10, 5, 0, 255], 5), entry([100, 9, 0, 255], 5), entry([50, 0, 0, 255], 5)];
        let mut b = ColorBox { index: 0, colors: 4, sum: 20, ..ColorBox::default() };
        let new = median_cut_box(&mut b, &mut hist, false).unwrap();
        assert_eq!(hist.iter().map(|e| e.color[0]).collect::<Vec<_>>(), vec![10, 50, 100, 200]);
        assert_eq!((b.colors, b.sum), (2, 10));
        assert_eq!((new.index, new.colors, new.sum), (2, 2, 10));
    }

    #[test]
    fn two_colours_two_boxes_either_way() {
        for cut in [1, 2] {
            let mut hist = vec![entry([0, 0, 0, 255], 30), entry([255, 255, 255, 255], 10)];
            let (map, said) = make_color_map(&mut hist, 2, 40, 4, cut, 3, false).unwrap();
            assert_eq!(said, "");
            assert_eq!(map.len(), 4);
            assert_eq!((map[0].color, map[1].color), ([0, 0, 0, 255], [255, 255, 255, 255]));
            assert_eq!(map[3].color, [0, 0, 0, 255]); // unused entries stay black
        }
        let mut hist = vec![entry([0, 0, 0, 255], 1)];
        assert_eq!(make_color_map(&mut hist, 1, 1, 0, 1, 1, false), Err(MapError::NoBoxes));
        assert_eq!(make_color_map(&mut hist, 1, 1, 1, 1, 9, false).unwrap().1, BAD_METHOD);
    }

    #[test]
    fn the_middle_of_a_box_takes_alphas_range_from_blue() {
        let hist = vec![entry([0, 0, 10, 100], 1), entry([0, 0, 50, 200], 1)];
        let boxes = [ColorBox { index: 0, colors: 2, sum: 2, ..ColorBox::default() }];
        let mut map = [HistEntry::default()];
        compute_box_color_representatives(&boxes, &mut map, 1, &hist, true).unwrap().unwrap();
        assert_eq!(map[0].color, [0, 0, 30, 75]); // (50 + 100) / 2, not (100 + 200) / 2
    }

    #[test]
    fn mapping_with_and_without_dithering() {
        let p = PictureRgba { width: 4, height: 1, ratio: 1.0, fields: 3, pad: 0, data: [[100u8, 100, 100, 255]; 4].concat() };
        let map = [entry([0, 0, 0, 255], 0), entry([255, 255, 255, 255], 0)];
        let opts = QuantOptions { colors: 2, bits: [8; 4] };
        assert_eq!(reduce_colors(&p, &opts, &map, 0, false).unwrap(), vec![0, 0, 0, 0]);
        // 100 of 255: the error builds up until a pixel is nearer to white
        assert_eq!(reduce_colors(&p, &opts, &map, 1, false).unwrap(), vec![0, 1, 0, 0]);
        assert_eq!(reduce_colors(&p, &opts, &map, 2, false), Err(Fault::Uninitialised));
        assert_eq!(compute_rgb_distance([1, 2, 3, 4], [2, 4, 6, 8]), 30);
    }
}
