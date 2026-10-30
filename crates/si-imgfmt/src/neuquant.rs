// SPDX-License-Identifier: MIT
//! The neural colour quantiser (finding 0014, "The neural quantiser").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `neuQuant.c` (0x085bd7e0–0x085be728).
//!
//! It is the published self-organising-map quantiser (a one-dimensional network of colours
//! trained on samples of the picture: `initnet`, `contest`, `altersingle`, `alterneigh`,
//! `learn`, `unbiasnet`), with every integer constant of the published program. Where this
//! build departs from it:
//! - the network's size is the number of colours asked for, not 256, and an optional fourth
//!   channel (alpha) is trained with the others;
//! - it is not trained on the picture but on **the histogram written out**: every colour of
//!   the histogram list as often as it occurs, colour after colour, in the list's order;
//! - at least **5,000** samples are taken, however small the picture, and the walk through
//!   the samples wraps round as often as it must (the published step wraps once);
//! - there is no index built over the network and no search in it: the trained colours go
//!   back through depth reduction, sorting and a repair of duplicates
//!   ([`si_neural_make_color_map`]), and pixels are mapped by `quant.c`'s own search.
//!
//! Nothing is random: the same histogram gives the same palette.
//!
//! The original keeps the network in globals; here it is the [`Net`] value. Progress reports
//! and the interrupt test (every hundredth sample, when asked for) are left out.

use crate::effect::Fault;
use crate::quant::HistEntry;
use si_x87::{Round, F80};
use std::cmp::Ordering;

/// The network and what belongs to it: the original's globals `netsize`, `network`, `bias`,
/// `freq`, `radpower`, `thepicture`, `lengthcount`, `samplefac`, `alphadec`,
/// `numColorChannels` and the flag `si_alphaOn`. (`netindex` and `maxnetpos` are set and
/// never used.)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Net {
    pub size: i32,
    /// `si_alphaOn`: a fourth channel is trained.
    pub alpha: bool,
    /// `numColorChannels`: bytes a sample, 3 or 4.
    pub channels: i32,
    /// One row a neuron: the channels (scaled by 16 while training), then the neuron's
    /// number once [`Net::unbiasnet`] has run. The original's rows are four words, five with
    /// alpha.
    pub network: Vec<[i32; 5]>,
    pub bias: Vec<i32>,
    pub freq: Vec<i32>,
    /// `size / 8` words.
    pub radpower: Vec<i32>,
    pub picture: Vec<u8>,
    pub lengthcount: i32,
    pub samplefac: i32,
    pub alphadec: i32,
}

/// The four primes of the sampling step.
pub const PRIMES: [i32; 4] = [499, 491, 487, 503];
/// The least number of samples [`Net::learn`] takes.
pub const MIN_SAMPLES: i32 = 5000;

fn div_toward_zero(v: i32, shift: u32) -> i32 {
    // the compiler's signed division by a power of two
    (if v < 0 { v.wrapping_add((1 << shift) - 1) } else { v }) >> shift
}

impl Net {
    fn row_len(&self) -> usize {
        if self.alpha {
            5
        } else {
            4
        }
    }

    /// Sets the network up [soft 0x085bd7e0 initnet]: neuron i starts at
    /// (i × 4096) / size in every channel (a grey ramp, scaled by 16), with frequency
    /// 65536 / size and no bias.
    pub fn initnet(size: i32, picture: Vec<u8>, lengthcount: i32, samplefac: i32, alpha: bool, channels: i32) -> Result<Net, Fault> {
        if size > 1 << 20 {
            return Err(Fault::BadSize);
        }
        let n = size.max(0) as usize;
        let mut net = Net { size, alpha, channels, network: vec![[0; 5]; n], bias: vec![0; n], freq: vec![0; n], radpower: vec![0; (size >> 3).max(0) as usize], picture, lengthcount, samplefac, alphadec: 0 };
        for i in 0..n {
            let v = ((i as i32) << 12) / size;
            net.network[i][..3].fill(v);
            if alpha {
                net.network[i][3] = v;
            }
            net.freq[i] = 0x10000 / size;
        }
        Ok(net)
    }

    /// Brings the channels back from their scale of 16 and writes each neuron's number
    /// behind them [soft 0x085bd93c unbiasnet].
    pub fn unbiasnet(&mut self) -> Result<(), Fault> {
        let ch = self.channels;
        let row_len = self.row_len();
        for i in 0..self.size.max(0) as usize {
            let row = self.network.get_mut(i).ok_or(Fault::ShortPlane)?;
            for c in 0..ch.max(0) as usize {
                let v = row.get_mut(c).ok_or(Fault::ShortPlane)?;
                *v >>= 4;
            }
            // with three channels and alpha on, this overwrites the alpha channel
            if ch < 0 || ch as usize >= row_len {
                return Err(Fault::ShortPlane);
            }
            row[ch as usize] = i as i32;
        }
        Ok(())
    }

    /// Finds the neuron for a sample [soft 0x085bd998 contest] and returns its number.
    ///
    /// The distance is the sum of absolute differences. The neuron returned is the one with
    /// the least distance less its bias / 4096; the one with the least plain distance has its
    /// frequency raised by 64 and its bias lowered by 65536. On the way every neuron's
    /// frequency loses its 1024th part, which is added (× 1024) to its bias. The first of
    /// equals wins both.
    pub fn contest(&mut self, v: [i32; 4]) -> Result<i32, Fault> {
        let (mut bestd, mut bestpos, mut bestbiasd, mut bestbiaspos) = (i32::MAX, -1i32, i32::MAX, -1i32);
        for i in 0..self.size.max(0) as usize {
            let (Some(n), true) = (self.network.get(i), i < self.bias.len() && i < self.freq.len()) else { return Err(Fault::ShortPlane) };
            let mut dist = 0i32;
            for c in 0..if self.alpha { 4 } else { 3 } {
                dist = dist.wrapping_add(n[c].wrapping_sub(v[c]).wrapping_abs());
            }
            if dist < bestd {
                bestd = dist;
                bestpos = i as i32;
            }
            let biasdist = dist.wrapping_sub(self.bias[i] >> 12);
            if biasdist < bestbiasd {
                bestbiasd = biasdist;
                bestbiaspos = i as i32;
            }
            let betafreq = self.freq[i] >> 10;
            self.freq[i] = self.freq[i].wrapping_sub(betafreq);
            self.bias[i] = self.bias[i].wrapping_add(betafreq.wrapping_mul(1024));
        }
        // with no neuron at all the original writes in front of its two arrays
        let at = usize::try_from(bestpos).map_err(|_| Fault::ShortPlane)?;
        self.freq[at] = self.freq[at].wrapping_add(64);
        self.bias[at] = self.bias[at].wrapping_sub(0x10000);
        Ok(bestbiaspos)
    }

    fn pull(&mut self, i: i32, v: [i32; 4], weight: i32, shift: u32) -> Result<(), Fault> {
        let alpha = self.alpha;
        let n = usize::try_from(i).ok().and_then(|i| self.network.get_mut(i)).ok_or(Fault::ShortPlane)?;
        for c in 0..if alpha { 4 } else { 3 } {
            n[c] = n[c].wrapping_sub(div_toward_zero(n[c].wrapping_sub(v[c]).wrapping_mul(weight), shift));
        }
        Ok(())
    }

    /// Moves neuron `i` towards the sample by `alpha` / 1024 of the way
    /// [soft 0x085bda70 altersingle]; the division rounds towards zero.
    pub fn altersingle(&mut self, alpha: i32, i: i32, v: [i32; 4]) -> Result<(), Fault> {
        self.pull(i, v, alpha, 10)
    }

    /// Moves the neighbours of neuron `i` within `rad` [soft 0x085bdb04 alterneigh]: the
    /// k-th neighbour on either side by `radpower[k]` / 2¹⁸ of the way. `radpower[0]` is
    /// never used.
    pub fn alterneigh(&mut self, rad: i32, i: i32, v: [i32; 4]) -> Result<(), Fault> {
        let lo = i.wrapping_sub(rad).max(-1);
        let hi = rad.wrapping_add(i).min(self.size);
        let (mut j, mut k, mut q) = (i.wrapping_add(1), i.wrapping_sub(1), 0usize);
        while j < hi || k > lo {
            q += 1;
            let a = *self.radpower.get(q).ok_or(Fault::ShortPlane)?;
            if j < hi {
                self.pull(j, v, a, 18)?;
                j += 1;
            }
            if k > lo {
                self.pull(k, v, a, 18)?;
                k -= 1;
            }
        }
        Ok(())
    }

    fn set_radpower(&mut self, rad: i32, alpha: i32) -> Result<(), Fault> {
        for i in 0..rad {
            let r2 = rad.wrapping_mul(rad);
            let v = (r2.wrapping_sub(i.wrapping_mul(i)) << 8) / r2;
            *self.radpower.get_mut(i as usize).ok_or(Fault::ShortPlane)? = v.wrapping_mul(alpha);
        }
        Ok(())
    }

    /// Trains the network [soft 0x085bdc8c learn].
    ///
    /// - Samples taken: `lengthcount / (samplefac × channels)`, but at least 5,000.
    /// - Start: alpha 1024, radius (size / 8) × 64; the neighbourhood is radius / 64, or
    ///   none when that is below 2.
    /// - Step through the samples: `channels` × 499 bytes, or × 491, 487, 503: the first of
    ///   these primes that does not divide `lengthcount`. Past the end the position comes
    ///   back by `lengthcount` as often as needed.
    /// - Every (samples / 100)-th sample: alpha loses its `alphadec`-th part
    ///   (`alphadec` = 30 + (samplefac − 1) / 3), the radius its 30th, and the neighbourhood
    ///   weights are made again: weight k = (((rad² − k²) × 256) / rad²) × alpha.
    ///
    /// Always returns true here; the original returns false when interrupted.
    pub fn learn(&mut self) -> Result<bool, Fault> {
        self.alphadec = self.samplefac.wrapping_sub(1) / 3 + 30;
        let per = self.samplefac.wrapping_mul(self.channels);
        if per == 0 || (self.lengthcount == i32::MIN && per == -1) {
            return Err(Fault::DivideError);
        }
        let mut samplepixels = self.lengthcount / per;
        if samplepixels < MIN_SAMPLES {
            samplepixels = MIN_SAMPLES;
        }
        let delta = samplepixels / 100;
        let mut alpha = 1024i32;
        let mut radius = (self.size >> 3) << 6;
        let mut rad = radius >> 6;
        if rad <= 1 {
            rad = 0;
        }
        self.set_radpower(rad, 1024)?;
        let len = self.lengthcount;
        let step = self.channels.wrapping_mul(*PRIMES[..3].iter().find(|&&p| len % p != 0).unwrap_or(&PRIMES[3]));
        if len <= 0 || step <= 0 || self.picture.len() < len as usize {
            // an empty sample makes the original's position loop run for ever
            return Err(Fault::BadSize);
        }
        let mut p = 0usize;
        let mut i = 0;
        while i < samplepixels {
            let px = |k: usize| self.picture.get(p + k).map(|&b| (b as i32) << 4).ok_or(Fault::ShortPlane);
            let v = [px(0)?, px(1)?, px(2)?, if self.alpha { px(3)? } else { 0 }];
            let j = self.contest(v)?;
            self.altersingle(alpha, j, v)?;
            if rad != 0 {
                self.alterneigh(rad, j, v)?;
            }
            i += 1;
            p += step as usize;
            while p >= len as usize {
                p -= len as usize;
            }
            if i % delta == 0 {
                if self.alphadec == 0 {
                    return Err(Fault::DivideError);
                }
                alpha -= alpha / self.alphadec;
                radius -= radius / 30;
                rad = radius >> 6;
                if rad <= 1 {
                    rad = 0;
                }
                self.set_radpower(rad, alpha)?;
            }
        }
        Ok(true)
    }
}

/// What [`neural_make_color_map`] prints when it cannot allocate the palette (no colours
/// asked for).
pub const NO_MAP: &str = "Error! unable to allocate ColorMap structre\n";

/// The sampling factor for a quality from 1 (fast) to 10 (every sample):
/// trunc(30 − (quality − 1) / 9 × 29 + 0.5), so 30 down to 1.
pub fn sample_factor(quality: i32) -> i32 {
    (F80::from_f64(30.0) - F80::from_i32(quality.wrapping_sub(1)) / F80::from_f64(9.0) * F80::from_f64(29.0) + F80::from_f64(0.5)).to_i32(Round::Zero)
}

/// A palette of `colors` entries trained on `pixels` samples of `channels` bytes each
/// [soft 0x085bdfe8 Neural_makeColorMap]: set-up, training, and one entry per neuron.
///
/// The samples hold blue, green, red (and alpha); an entry is the neuron's third, second
/// and first channel, so red, green, blue, then alpha or 255. `Ok(Err(text))` is the case
/// in which the routine prints [`NO_MAP`] and returns nothing. The network is returned too:
/// the original leaves it in its globals.
pub fn neural_make_color_map(sample: Vec<u8>, pixels: i32, colors: i32, quality: i32, alpha: bool, channels: i32) -> Result<Result<(Vec<HistEntry>, Net), &'static str>, Fault> {
    let mut net = Net::initnet(colors, sample, pixels.wrapping_mul(channels), sample_factor(quality), alpha, channels)?;
    net.learn()?;
    net.unbiasnet()?;
    if colors <= 0 {
        return Ok(Err(NO_MAP));
    }
    let map = net.network.iter().map(|n| HistEntry { color: [n[2] as u8, n[1] as u8, n[0] as u8, if alpha { n[3] as u8 } else { 0xff }], count: 0, extra: 0 }).collect();
    Ok(Ok((map, net)))
}

/// Orders palette entries by red, green, blue (and alpha when it counts), **largest first**
/// [soft 0x085be104 colormapCompare].
pub fn colormap_compare(a: &HistEntry, b: &HistEntry, alpha: bool) -> Ordering {
    for c in 0..if alpha { 4 } else { 3 } {
        match b.color[c].cmp(&a.color[c]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// Orders histogram entries by their count, largest first [soft 0x085be168 histoCompare].
pub fn histo_compare(a: &HistEntry, b: &HistEntry) -> Ordering {
    b.count.cmp(&a.count)
}

/// The C library's `bsearch` over the palette with [`colormap_compare`]: is the key there.
/// The palette is no longer in order once duplicates have been replaced; the search goes
/// its way all the same, and so does this.
fn found(key: &HistEntry, map: &[HistEntry], alpha: bool) -> bool {
    let (mut lo, mut hi) = (0usize, map.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        match colormap_compare(key, &map[mid], alpha) {
            Ordering::Less => hi = mid,
            Ordering::Greater => lo = mid + 1,
            Ordering::Equal => return true,
        }
    }
    false
}

/// Chooses `wanted` colours for a histogram with the neural method
/// [soft 0x085be18c si_neuralMakeColorMap]. `bits` are the depths of the histogram's
/// colours; `pixels` the sum of its counts.
///
/// - With no more colours than wanted, the palette is the histogram's colours in its order
///   (alpha 255 when alpha does not count), then black entries with alpha 255.
/// - Otherwise the samples are the histogram written out: each colour `count` times as
///   blue, green, red (and alpha), each shifted up to 8 bits (`<< (8 − depth)`: the low bits
///   are zero, not a repeat of the high ones). [`neural_make_color_map`] trains on them.
/// - The trained colours are brought back to the histogram's depths:
///   `rint(value / pow(2, 8 − depth))`, ties to even. The palette is sorted
///   ([`colormap_compare`]: largest first) and equal neighbours are marked.
/// - If there are duplicates, the histogram is sorted by count, largest first (**its order
///   is changed for the caller**), and each duplicate is replaced by the most frequent
///   colour that a binary search does not find in the palette.
/// - An entry's count is 0, or −1 for a duplicate nothing was found for.
///
/// Both sorts are the C library's `qsort`, which keeps equal entries in order.
/// `pow` is asked through [`si_x87::trig::pow`], so a check can replay the library's answers.
pub fn si_neural_make_color_map(hist: &mut [HistEntry], colors: i32, pixels: u32, bits: &[i32; 4], wanted: i32, quality: i32, alpha: bool) -> Result<Option<Vec<HistEntry>>, Fault> {
    let n = colors.max(0) as usize;
    if hist.len() < n {
        return Err(Fault::ShortPlane);
    }
    if wanted >= colors {
        if wanted <= 0 {
            return Ok(None);
        }
        if wanted > 1 << 20 {
            return Err(Fault::BadSize);
        }
        let mut map = vec![HistEntry { color: [0, 0, 0, 0xff], count: 0, extra: 0 }; wanted as usize];
        for (m, h) in map.iter_mut().zip(&hist[..n]) {
            m.color = [h.color[0], h.color[1], h.color[2], if alpha { h.color[3] } else { 0xff }];
        }
        return Ok(Some(map));
    }
    let shift = bits.map(|b| 8i32.wrapping_sub(b));
    let channels = if alpha { 4 } else { 3 };
    let total = (channels as u32).wrapping_mul(pixels) as usize;
    if total > 1 << 28 {
        return Err(Fault::BadSize);
    }
    let mut sample = Vec::with_capacity(total);
    for h in &hist[..n] {
        for _ in 0..h.count.max(0) {
            if sample.len() + channels > total {
                return Err(Fault::ShortPlane); // more samples than the buffer has room for
            }
            let up = |c: usize| ((h.color[c] as u32).wrapping_shl(shift[c] as u32)) as u8;
            sample.extend_from_slice(&[up(2), up(1), up(0)]);
            if alpha {
                sample.push(up(3));
            }
        }
    }
    if sample.len() != total {
        // the rest of the buffer is memory nobody set, and the training samples it
        return Err(Fault::Uninitialised);
    }
    let mut map = match neural_make_color_map(sample, pixels as i32, wanted, quality, alpha, channels as i32)? {
        Ok((map, _)) => map,
        Err(_) => return Ok(None),
    };
    let power: Vec<f64> = (0..channels).map(|c| si_x87::trig::pow(2.0, shift[c] as f64).to_f64()).collect();
    for m in map.iter_mut() {
        for c in 0..channels {
            let q = (F80::from_i32(m.color[c] as i32) / F80::from_f64(power[c])).to_f64();
            m.color[c] = F80::from_f64(q).round_int(Round::Nearest).to_i32(Round::Zero) as u8;
        }
    }
    map.sort_by(|a, b| colormap_compare(a, b, alpha));
    let mut unique = 1;
    for i in 1..map.len() {
        if map[i].color == map[i - 1].color {
            map[i].count = -1;
        } else {
            map[i].count = 0;
            unique += 1;
        }
    }
    if unique != wanted {
        hist[..n].sort_by(histo_compare);
        let mut k = 0usize;
        'entries: for i in 1..map.len() {
            if map[i].count != -1 {
                continue;
            }
            loop {
                // past the histogram's colours the original reads on in its list
                let key = *hist.get(k).filter(|_| k < n).ok_or(Fault::ShortPlane)?;
                if !found(&key, &map, alpha) {
                    break;
                }
                k += 1;
                if k == n {
                    break 'entries;
                }
            }
            map[i].color = hist[k].color;
            k += 1;
        }
    }
    Ok(Some(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sampling_factor_runs_from_thirty_to_one() {
        assert_eq!((sample_factor(1), sample_factor(10), sample_factor(5)), (30, 1, 17));
    }

    #[test]
    fn a_new_network_is_a_grey_ramp() {
        let net = Net::initnet(16, vec![0; 12], 12, 1, false, 3).unwrap();
        assert_eq!(net.network[8], [2048, 2048, 2048, 0, 0]);
        assert_eq!((net.freq[0], net.radpower.len()), (4096, 2));
    }

    #[test]
    fn few_colours_are_taken_as_they_are() {
        let mut hist = vec![HistEntry { color: [1, 2, 3, 4], count: 5, extra: 0 }];
        let map = si_neural_make_color_map(&mut hist, 1, 5, &[8; 4], 3, 1, false).unwrap().unwrap();
        assert_eq!(map.iter().map(|e| e.color).collect::<Vec<_>>(), vec![[1, 2, 3, 255], [0, 0, 0, 255], [0, 0, 0, 255]]);
    }

    #[test]
    fn two_far_colours_and_a_palette_of_two() {
        // three colours, two entries: the network has to choose; black and white dominate
        let mut hist = vec![
            HistEntry { color: [0, 0, 0, 255], count: 600, extra: 0 },
            HistEntry { color: [255, 255, 255, 255], count: 600, extra: 0 },
            HistEntry { color: [128, 128, 128, 255], count: 1, extra: 0 },
        ];
        let map = si_neural_make_color_map(&mut hist, 3, 1201, &[8; 4], 2, 10, false).unwrap().unwrap();
        assert_eq!(map.len(), 2);
        assert!(map[0].color[0] > 200 && map[1].color[0] < 50, "{map:?}");
        assert_eq!(colormap_compare(&map[0], &map[1], false), Ordering::Less);
    }
}
