// SPDX-License-Identifier: MIT
//! Colour reduction from end to end: the routines of `quant.c` that string histogram,
//! palette and mapping together, and the palette utilities around them (finding 0014,
//! "Colour reduction, the whole run").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `quant.c` (0x085af7a0–0x085b45c4); its first half
//! (histogram, boxes, mapping) is in [`crate::quant`].
//!
//! The original keeps the state of a reduction in globals: `options` (the number of colours
//! and the four depths the palette is being made at, with their masks), `si_countReduce`
//! (how often each index is used in the last picture reduced) and `si_alphaOn`. Here they
//! are the [`Reducer`] value, and the routines are its methods.
//!
//! Left out throughout, as in [`crate::quant`]: the progress reports and the test for an
//! interrupt key that the original makes when its `short` progress argument is non-zero.

use crate::dgt::{DgPalette, DgPicture, ENTRY_LOCKED};
use crate::effect::Fault;
use crate::neuquant::si_neural_make_color_map;
use crate::quant::{self, color_equiv_table, compute_rgb_distance, ColorHash, HistEntry, MapError, QuantOptions, ENTRY_USABLE};
use si_picture::PictureRgba;
use si_x87::{Round, F80};

/// The original's `SI_QuantizeParameters`, four words passed by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantizeParameters {
    /// 1: the neural quantiser; anything else: boxes.
    pub method: i32,
    /// Neural quality, 1 (fast) to 10.
    pub quality: i32,
    /// Box rule: 1 median, 2 variance.
    pub cut: i32,
    /// Box representative: 1 middle, 2 mean of colours, 3 mean of pixels.
    pub representative: i32,
}

/// The globals of a colour reduction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reducer {
    /// `options` (0x08829fc0): the colours being made, and at +4 their depths.
    pub options: QuantOptions,
    /// At +0x14: 2^depth − 1 per channel.
    pub masks: [i32; 4],
    /// `si_countReduce`: for every palette entry its number and how many pixels use it.
    pub count_reduce: Option<Vec<[u32; 2]>>,
    /// `si_alphaOn`: alpha counts.
    pub alpha: bool,
}

/// How many entries of a palette are locked: the loop four callers have inline. No palette,
/// none.
fn locked(pal: Option<&DgPalette>) -> i32 {
    pal.map_or(0, |p| p.locked())
}

fn mask(bits: i32) -> i32 {
    1i32.wrapping_shl(bits as u32).wrapping_sub(1)
}

/// Fills a palette from a colour list: entry i = trunc(colour × scale) per channel, the
/// scale a double (the loop three routines share).
fn fill(pal: &mut DgPalette, map: &[HistEntry], n: i32, scale: &[f64; 4], lock: bool) -> Result<(), Fault> {
    for i in 0..n.max(0) as usize {
        // one routine reads an entry more than its list has (see `compute_palette`)
        let Some(m) = map.get(i) else { break };
        let v = [0, 1, 2, 3].map(|c| (F80::from_i32(m.color[c] as i32) * F80::from_f64(scale[c])).to_i32(Round::Zero));
        pal.set_entry(i as u32, v)?;
        if lock && m.count >= 0 {
            pal.entries.get_mut(i).ok_or(Fault::NullPlane)?.flags |= ENTRY_LOCKED;
        }
    }
    Ok(())
}

impl Reducer {
    pub fn new(alpha: bool) -> Reducer {
        Reducer { options: QuantOptions { colors: 0, bits: [0; 4] }, masks: [0; 4], count_reduce: None, alpha }
    }

    fn set_masks(&mut self) {
        self.masks = self.options.bits.map(mask);
    }

    /// A palette's entries as the list the mapping works on
    /// [soft 0x085af7a0 SI_convertColorMap]: the colours as they are, and in the third word's low byte the
    /// entry's flags, with 0x10 (usable) added when the entry's mark is not zero. Sets the
    /// globals to the palette's count and depths, also when the palette is empty and
    /// nothing is returned.
    pub fn convert_color_map(&mut self, pal: &DgPalette) -> Result<Option<Vec<HistEntry>>, Fault> {
        let n = pal.count as i32;
        let map = if n > 0 {
            let entries = pal.entries.get(..n as usize).ok_or(Fault::NullPlane)?;
            Some(entries.iter().map(|e| HistEntry { color: e.color, count: 0, extra: (e.flags | if e.mark != 0 { ENTRY_USABLE as u8 } else { 0 }) as i32 }).collect())
        } else {
            None
        };
        self.options = QuantOptions { colors: n, bits: pal.bits };
        self.set_masks();
        Ok(map)
    }

    /// A palette for one picture [soft 0x085af858 SI_generateColorMap]: histogram, then the
    /// neural method or the boxes.
    ///
    /// With `reserve` one colour fewer is made (entry 0 is added later). `bits` are the
    /// depths wanted; they come back lowered if the picture had more than 65,536 colours at
    /// them. The second value is what went to standard error.
    pub fn generate_color_map(&mut self, pic: &PictureRgba, colors: i32, reserve: i32, bits: &mut [i32; 4], q: QuantizeParameters) -> Result<(Option<Vec<HistEntry>>, &'static str), Fault> {
        self.options = QuantOptions { colors: if reserve != 0 { colors.wrapping_sub(1) } else { colors }, bits: *bits };
        let (mut hist, n) = quant::make_color_hist_vector_list(pic, &mut self.options.bits, self.alpha)?;
        let pixels = (pic.width as i32).wrapping_mul(pic.height as i32) as u32;
        let (map, said) = if q.method == 1 {
            (si_neural_make_color_map(&mut hist, n, pixels, &self.options.bits, self.options.colors, q.quality, self.alpha)?, "")
        } else {
            match quant::make_color_map(&mut hist, n, pixels, self.options.colors, q.cut, q.representative, self.alpha) {
                Ok((map, said)) => (Some(map), said),
                Err(MapError::NoBoxes) => (None, "Error, unable to allocate boxVector structure\n"),
                Err(MapError::BadCut) => (None, ""),
                Err(MapError::Fault(f)) => return Err(f),
            }
        };
        self.set_masks();
        *bits = self.options.bits;
        Ok((map, said))
    }

    /// Maps a picture to a colour list and returns the index picture with its palette
    /// [soft 0x085af95c SI_pictureRgb2ColorIndex].
    ///
    /// - The palette has as many entries as the globals say, at the depths `bits`; entry i is
    ///   trunc(colour × (2^bits − 1) / (2^depth of the list − 1)), the quotient a double.
    /// - With `reserve`, if the palette has fewer than `most` entries: one entry is added,
    ///   every entry moves up by one (all of it: flags and counts too, so entry 1 is a
    ///   copy of the old entry 0 in those), entry 0 becomes black with full alpha, and
    ///   **every index is raised by one**.
    /// - The use counts are made afresh ([`Reducer::reset_global_color_count`]).
    pub fn picture_rgb2color_index(&mut self, pic: &PictureRgba, map: &[HistEntry], dither: i16, reserve: i32, bits: &[i32; 4], most: i32) -> Result<Option<DgPicture>, Fault> {
        let mut dg = DgPicture::allocate(pic.width as i32, pic.height as i32, self.options.colors, bits);
        let ix = quant::reduce_colors(pic, &self.options, map, dither, self.alpha)?;
        match dg.data.as_mut() {
            Some(d) if d.len() >= ix.len() => d[..ix.len()].copy_from_slice(&ix),
            _ if ix.is_empty() => {}
            _ => return Err(Fault::BadSize),
        }
        let mut scale = [0f64; 4];
        for c in 0..4 {
            scale[c] = (F80::from_i32(mask(bits[c])) / F80::from_i32(mask(self.options.bits[c]))).to_f64();
        }
        let pal = dg.palette.as_mut().unwrap();
        fill(pal, map, self.options.colors, &scale, false)?;
        if map.len() < self.options.colors.max(0) as usize {
            return Err(Fault::ShortPlane);
        }
        if reserve != 0 && pal.count < most as u32 {
            pal.resize(pal.count.wrapping_add(1) as i32)?;
            for i in (1..pal.count as usize).rev() {
                pal.entries[i] = pal.entries[i - 1];
            }
            pal.set_entry(0, [0, 0, 0, 0xff])?;
            let n = dg.width as usize * dg.height as usize;
            if n != 0 {
                for v in dg.data.as_mut().ok_or(Fault::NullPlane)?.iter_mut().take(n) {
                    *v = v.wrapping_add(1);
                }
            }
        }
        self.reset_global_color_count(&dg)?;
        Ok(Some(dg))
    }

    /// Takes repeated colours out of an index picture's palette
    /// [soft 0x085afba0 si_paletteFilterRedundancy].
    ///
    /// Nothing happens when every entry has a colour of its own. Otherwise a new palette of
    /// the same size and depths is made with the different colours first, **in the order of
    /// the colour hash table** (bucket by bucket, the later of two entries in one bucket
    /// first), then black entries with full alpha; every pixel gets its colour's new
    /// number. Locks, marks and the three entry numbers of the old palette are not carried
    /// over. Two colours are the same by the hash table's rule (see
    /// [`ColorHash::check`]): with alpha off, colours that differ only in alpha are still two
    /// unless they share a bucket.
    pub fn palette_filter_redundancy(&mut self, dg: Option<&mut DgPicture>) -> Result<(), Fault> {
        let Some(dg) = dg else { return Ok(()) };
        let mut table = ColorHash::default();
        let n = dg.palette.as_ref().ok_or(Fault::NullPlane)?.count as i32;
        self.reset_global_color_count(dg)?;
        let old = dg.palette.as_ref().unwrap();
        let mut different = 0;
        for i in 0..n.max(0) as usize {
            let c = old.entries.get(i).ok_or(Fault::NullPlane)?.color;
            if table.check(c, self.alpha) == -1 {
                table.add(c, 0);
                different += 1;
            }
        }
        if different >= n {
            return Ok(());
        }
        let mut new = DgPalette::allocate(n, &old.bits);
        let colors = table.number();
        for (k, c) in colors.iter().enumerate() {
            new.set_entry(k as u32, c.map(|v| v as i32))?;
        }
        for k in colors.len()..n as usize {
            new.set_entry(k as u32, [0, 0, 0, 0xff])?;
        }
        let area = dg.width as usize * dg.height as usize;
        if area != 0 {
            let data = dg.data.as_mut().ok_or(Fault::NullPlane)?.get_mut(..area).ok_or(Fault::ShortPlane)?;
            for v in data.iter_mut() {
                let c = old.entries.get(*v as usize).ok_or(Fault::ShortPlane)?.color;
                *v = table.check(c, self.alpha) as u16;
            }
        }
        dg.palette = Some(new);
        self.reset_global_color_count(dg)
    }

    /// How many different palette colours the pixels of an index picture use
    /// [soft 0x085afe80 si_paletteCountEffectiveEntries], by the hash table's rule.
    pub fn palette_count_effective_entries(&self, dg: Option<&DgPicture>) -> Result<i32, Fault> {
        let Some(dg) = dg else { return Ok(0) };
        let mut table = ColorHash::default();
        let area = dg.width as usize * dg.height as usize;
        let mut n = 0;
        if area != 0 {
            let pal = dg.palette.as_ref().ok_or(Fault::NullPlane)?;
            for &v in dg.data.as_ref().ok_or(Fault::NullPlane)?.get(..area).ok_or(Fault::ShortPlane)? {
                let c = pal.entries.get(v as usize).ok_or(Fault::ShortPlane)?.color;
                if table.check(c, self.alpha) == -1 {
                    table.add(c, 0);
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// Counts the different colours of a packed picture
    /// [soft 0x085b274c si_totalCouleursRGB]: red, green and blue alone when alpha does not count (a bit per
    /// colour in a table of 2 MB), all four bytes when it does (the hash table).
    pub fn total_couleurs_rgb(&self, pic: &PictureRgba) -> Result<i32, Fault> {
        let n = (pic.height as i32).wrapping_mul(pic.width as i32);
        if n <= 0 {
            return Ok(0);
        }
        let px = pic.data.get(..4 * n as usize).ok_or(Fault::ShortPlane)?;
        let mut count = 0;
        if !self.alpha {
            let mut seen = vec![0u8; 0x200000];
            for p in px.chunks_exact(4) {
                let key = (p[0] as usize) << 16 | (p[1] as usize) << 8 | p[2] as usize;
                if seen[key >> 3] >> (key & 7) & 1 == 0 {
                    seen[key >> 3] |= 1 << (key & 7);
                    count += 1;
                }
            }
        } else {
            let mut table = ColorHash::default();
            for p in px.chunks_exact(4) {
                let c = [p[0], p[1], p[2], p[3]];
                if table.check(c, true) == -1 {
                    table.add(c, 0);
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Maps a picture to a given palette [soft 0x085b2ecc SI_reduceImage]. The new palette
    /// takes the given one's three entry numbers and every entry's mark; a lock on entry i
    /// is carried to entry i (i + 1 with `reserve`, where the last entry's lock is dropped).
    pub fn reduce_image(&mut self, pic: Option<&PictureRgba>, reserve: i32, dither: i32, pal: Option<&DgPalette>) -> Result<Option<DgPicture>, Fault> {
        let (Some(pic), Some(pal)) = (pic, pal) else { return Ok(None) };
        let bits = pal.bits;
        let mut n = pal.count as i32;
        let Some(map) = self.convert_color_map(pal)? else { return Ok(None) };
        let Some(mut dg) = self.picture_rgb2color_index(pic, &map, dither as i16, reserve, &bits, n)? else { return Ok(None) };
        let new = dg.palette.as_mut().unwrap();
        new.sel = pal.sel;
        for i in 0..n.max(0) as usize {
            new.entries.get_mut(i).ok_or(Fault::ShortPlane)?.mark = pal.entries[i].mark;
        }
        if pal.locked() > 0 {
            if reserve != 0 {
                n -= 1;
            }
            for i in 0..n.max(0) as usize {
                if pal.entries[i].flags & ENTRY_LOCKED != 0 {
                    new.entries.get_mut(if reserve != 0 { i + 1 } else { i }).ok_or(Fault::ShortPlane)?.flags |= ENTRY_LOCKED;
                }
            }
        }
        Ok(Some(dg))
    }

    /// Reduces a picture to `colors` colours [soft 0x085b3060 SI_colorReduce]: with
    /// `use_palette` to the given palette's colours (count and depths are then the
    /// palette's, and `bits` is set to them), otherwise to a palette made for the picture
    /// ([`Reducer::generate_color_map`]). Either way the given palette's locked entries are
    /// worked in ([`palette_integrate_locks`]) and the entries that came from a locked one
    /// are locked in the result.
    pub fn color_reduce(&mut self, pic: &PictureRgba, mut colors: i32, bits: &mut [i32; 4], pal: Option<&DgPalette>, dither: i16, reserve: i16, q: QuantizeParameters, use_palette: bool) -> Result<(Option<DgPicture>, &'static str), Fault> {
        let mut said = "";
        let mut map = if !use_palette {
            let (map, text) = self.generate_color_map(pic, colors, reserve as i32, bits, q)?;
            said = text;
            match map {
                Some(m) => m,
                None => return Ok((None, said)),
            }
        } else {
            let pal = pal.ok_or(Fault::NullPlane)?;
            let Some(map) = self.convert_color_map(pal)? else { return Ok((None, said)) };
            colors = pal.count as i32;
            *bits = pal.bits;
            map
        };
        let locks = locked(pal);
        palette_integrate_locks(Some(&mut map), colors, bits, pal, locks, reserve)?;
        let Some(mut dg) = self.picture_rgb2color_index(pic, &map, dither, reserve as i32, bits, colors)? else { return Ok((None, said)) };
        if locks > 0 {
            if reserve != 0 {
                colors -= 1;
            }
            let new = dg.palette.as_mut().unwrap();
            for i in 0..colors.max(0) as usize {
                if map.get(i).ok_or(Fault::ShortPlane)?.count >= 0 {
                    new.entries.get_mut(if reserve != 0 { i + 1 } else { i }).ok_or(Fault::ShortPlane)?.flags |= ENTRY_LOCKED;
                }
            }
        }
        Ok((Some(dg), said))
    }

    /// For every palette entry its number and the number of pixels that use it
    /// [soft 0x085b267c calculeColorCount]; nothing for an empty palette.
    pub fn calcule_color_count(dg: &DgPicture) -> Result<Option<Vec<[u32; 2]>>, Fault> {
        let n = dg.palette.as_ref().ok_or(Fault::NullPlane)?.count;
        if n == 0 || n > crate::dgt::MAX_ENTRIES {
            return Ok(None);
        }
        let mut out: Vec<[u32; 2]> = (0..n).map(|i| [i, 0]).collect();
        let area = dg.width as usize * dg.height as usize;
        if area != 0 {
            for &v in dg.data.as_ref().ok_or(Fault::NullPlane)?.get(..area).ok_or(Fault::ShortPlane)? {
                let e = out.get_mut(v as usize).ok_or(Fault::ShortPlane)?;
                e[1] = e[1].wrapping_add(1);
            }
        }
        Ok(Some(out))
    }

    /// Makes the global use counts afresh for a picture
    /// [soft 0x085b2710 si_resetGlobalColorCount].
    pub fn reset_global_color_count(&mut self, dg: &DgPicture) -> Result<(), Fault> {
        self.count_reduce = None;
        self.count_reduce = Reducer::calcule_color_count(dg)?;
        Ok(())
    }

    /// One palette for several pictures [soft 0x085b298c SI_computePalette].
    ///
    /// `load` stands for `SI_imageLoad` (the image file classes are not ported): it is
    /// called once per name, in order, and returns the picture or nothing. A picture that
    /// does not load is passed over with the status message in the result's second value.
    ///
    /// The histograms of all pictures are added up in one table, a colour list is made from
    /// it as for one picture, the given palette's locks are worked in, and a palette of
    /// `colors` entries at the depths `bits` is filled from the list.
    ///
    /// With `reserve` the list has one entry fewer than the palette: the original fills the
    /// last entry from the memory behind its list. Here that entry stays as allocated.
    /// Nothing is returned when no picture loaded or the list could not be made.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_palette(&mut self, names: &[&[u8]], load: &mut dyn FnMut(&[u8]) -> Option<PictureRgba>, colors: i32, bits: &mut [i32; 4], q: QuantizeParameters, reserve: i32, pal: Option<&DgPalette>) -> Result<(Option<DgPalette>, Vec<String>), Fault> {
        let mut said = Vec::new();
        self.options = QuantOptions { colors: if reserve == 0 { colors } else { colors.wrapping_sub(1) }, bits: *bits };
        let (mut table, mut count, mut pixels) = (None::<ColorHash>, 0i32, 0i32);
        for name in names {
            match load(name) {
                None => said.push(format!("Color Reduction: Computing Palette: Skipping texture file \"{}\": Cannot read", String::from_utf8_lossy(name))),
                Some(p) => {
                    pixels = pixels.wrapping_add((p.width as i32).wrapping_mul(p.height as i32));
                    table = Some(quant::make_color_hash_table(&p, &mut self.options.bits, &mut count, table.take(), self.alpha)?);
                }
            }
        }
        let Some(table) = table else { return Ok((None, said)) };
        let mut hist = table.to_list();
        self.set_masks();
        *bits = self.options.bits;
        let map = if q.method == 1 {
            si_neural_make_color_map(&mut hist, count, pixels as u32, &self.options.bits, self.options.colors, q.quality, self.alpha)?
        } else {
            match quant::make_color_map(&mut hist, count, pixels as u32, self.options.colors, q.cut, q.representative, self.alpha) {
                Ok((map, _)) => Some(map),
                Err(MapError::Fault(f)) => return Err(f),
                Err(_) => None,
            }
        };
        let Some(mut map) = map else { return Ok((None, said)) };
        let locks = locked(pal);
        palette_integrate_locks(Some(&mut map), colors, bits, pal, locks, reserve as i16)?;
        let mut out = DgPalette::allocate(colors, bits);
        let mut scale = [0f64; 4];
        for c in 0..4 {
            scale[c] = (F80::from_i32(mask(bits[c])) / F80::from_i32(mask(self.options.bits[c]))).to_f64();
        }
        fill(&mut out, &map, colors, &scale, locks != 0)?;
        Ok((Some(out), said))
    }

    /// A palette of 8 bits a channel made from one picture, with a given palette's locks
    /// worked in [soft 0x085bd520 SI_getPaletteFromPictureRGBA] (`getPalette.c`).
    ///
    /// `asked` is what the dialog `SI_getPaletteParameters` returned (it is not ported): the
    /// number of colours, the depths to quantise at and the method; `None` when the user
    /// cancelled, and then nothing is made. Entry i is trunc(colour × double(255 / (2^depth
    /// − 1))); entries that came from a locked one are locked.
    pub fn get_palette_from_picture_rgba(&mut self, pic: &PictureRgba, pal: Option<&DgPalette>, asked: Option<(i32, [i32; 4], QuantizeParameters)>) -> Result<(Option<DgPalette>, &'static str), Fault> {
        let Some((colors, mut bits, q)) = asked else { return Ok((None, "")) };
        let (map, said) = self.generate_color_map(pic, colors, 0, &mut bits, q)?;
        let Some(mut map) = map else { return Ok((None, said)) };
        let locks = locked(pal);
        palette_integrate_locks(Some(&mut map), colors, &bits, pal, locks, 0)?;
        let mut out = DgPalette::allocate(colors, &[8; 4]);
        let scale = bits.map(|b| (F80::from_f64(255.0) / F80::from_i32(mask(b))).to_f64());
        if map.len() < colors.max(0) as usize {
            return Err(Fault::ShortPlane);
        }
        fill(&mut out, &map, colors, &scale, locks != 0)?;
        Ok((Some(out), said))
    }
}

/// Works a palette's locked entries into a colour list of `n` entries
/// [soft 0x085b3a2c si_paletteIntegrateLocks]. Returns false, with every entry's count set
/// to −1, when there are more locked entries than room; true otherwise.
///
/// `bits` are the depths of the list's colours, `locks` the number of locked entries of
/// `pal`. With `reserve` the list is taken to be one shorter. On return an entry's count is
/// 1 when it holds a locked colour and −1 when not (the callers lock the palette entries
/// they make from the former).
///
/// - Nothing locked, or no palette: the counts are set to −1 and that is all.
/// - As many locks as entries: the list becomes the palette's first `n` colours, brought to
///   the list's depths, with the palette's flags; **the counts stay −1**.
/// - Otherwise each locked entry, in the palette's order, takes the nearest list entry not
///   taken yet (squared distance over four channels, the first of equals). A list entry so
///   taken gets the locked colour and the palette's flags, and is moved to the place the
///   locked entry has in the palette if that place is inside the list; the entries left
///   over fill the free places in their old order. With `reserve` every locked entry then
///   changes places with the entry in front of it, front to back (the list is about to be
///   moved up by one for the reserved entry 0).
pub fn palette_integrate_locks(map: Option<&mut Vec<HistEntry>>, n: i32, bits: &[i32; 4], pal: Option<&DgPalette>, locks: i32, reserve: i16) -> Result<bool, Fault> {
    let Some(map) = map else { return Ok(false) };
    let n = if reserve != 0 { n.wrapping_sub(1) } else { n };
    let len = n.max(0) as usize;
    if map.len() < len {
        return Err(Fault::ShortPlane);
    }
    for m in map.iter_mut().take(len) {
        m.count = -1;
    }
    let Some(pal) = pal else { return Ok(true) };
    if n < locks {
        return Ok(false);
    }
    if locks == 0 {
        return Ok(true);
    }
    let mut luts = Vec::with_capacity(4);
    for c in 0..4 {
        luts.push(color_equiv_table(pal.bits[c], bits[c])?);
    }
    // a palette colour at the list's depths
    let lookup = |i: usize| -> Result<[u8; 4], Fault> {
        let e = pal.entries.get(i).ok_or(Fault::ShortPlane)?;
        let mut out = [0u8; 4];
        for c in 0..4 {
            out[c] = *luts[c].get(e.color[c] as usize).ok_or(Fault::ShortPlane)?;
        }
        Ok(out)
    };
    let low = |v: i32, b: u8| (v & !0xff) | b as i32;
    if locks == n {
        for i in 0..len {
            map[i].color = lookup(i)?;
            map[i].extra = low(map[i].extra, pal.entries[i].flags);
        }
        return Ok(true);
    }
    // from here on the original reads entry 0 whether or not the list has one
    if len == 0 {
        return Err(Fault::ShortPlane);
    }
    let held = map[0].extra & 1 != 0 && reserve != 0;
    if held {
        map[0].extra &= !1;
    }
    for j in 0..pal.count as usize {
        if pal.entries.get(j).ok_or(Fault::ShortPlane)?.flags & ENTRY_LOCKED == 0 {
            continue;
        }
        let want = lookup(j)?.map(|v| v as i32);
        let (mut best, mut at) = (i32::MAX, -1i32);
        for (i, m) in map.iter().take(len).enumerate() {
            if m.count < 0 {
                let d = compute_rgb_distance(want, m.color.map(|v| v as i32));
                if d < best {
                    at = i as i32;
                    best = d;
                }
            }
        }
        if at >= 0 {
            map[at as usize].count = j as i32;
        }
    }
    if held {
        map[0].extra |= 1;
    }
    let mut tmp = vec![HistEntry::default(); len];
    for i in 0..len {
        if map[i].count == -1 {
            tmp[i] = map[i];
        } else {
            let j = map[i].count as usize;
            tmp[i] = HistEntry { color: lookup(j)?, count: map[i].count, extra: pal.entries[j].flags as i32 };
        }
    }
    for m in map.iter_mut().take(len) {
        m.count = -1;
    }
    for t in tmp.iter_mut() {
        if t.count >= 0 && t.count < n {
            map[t.count as usize] = *t;
            t.count = -2;
        }
    }
    let mut k = 0usize;
    for t in &tmp {
        if t.count != -2 {
            while map.get(k).filter(|_| k < len).ok_or(Fault::ShortPlane)?.count >= 0 {
                k += 1;
            }
            map[k] = *t;
            map[k].count = 0;
        }
    }
    for m in map.iter_mut().take(len) {
        m.count = if m.extra & 1 != 0 { 1 } else { -1 };
    }
    if reserve != 0 && n > 1 {
        for i in 0..len - 1 {
            if map[i + 1].count >= 0 {
                map.swap(i, i + 1);
            }
        }
    }
    Ok(true)
}

/// What `SI_savePalette` does with a palette, by the kind asked for
/// [soft 0x085b3960 SI_savePalette]: `pal` the text file ([`crate::dgt::write_user_pal`]),
/// `dgt` a `.dgt` file without pixels ([`crate::dgt::write_dgt_palette`]); any other kind,
/// an empty name or no picture: nothing, and the call fails. The kind is compared whole.
pub fn save_palette(name: Option<&[u8]>, kind: Option<&[u8]>, dg: Option<&DgPicture>) -> Result<Option<Vec<u8>>, Fault> {
    let (Some(name), Some(kind), Some(dg)) = (name, kind, dg) else { return Ok(None) };
    if name.is_empty() || kind.is_empty() {
        return Ok(None);
    }
    match kind {
        b"pal" => Ok(Some(crate::dgt::write_user_pal(dg.palette.as_ref().ok_or(Fault::NullPlane)?)?)),
        b"dgt" => Ok(Some(crate::dgt::write_dgt_palette(dg)?)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(px: &[[u8; 4]], w: i16) -> PictureRgba {
        PictureRgba { width: w, height: (px.len() / w as usize) as i16, ratio: 1.0, fields: 3, pad: 0, data: px.concat() }
    }

    const BOXES: QuantizeParameters = QuantizeParameters { method: 2, quality: 1, cut: 1, representative: 3 };

    #[test]
    fn a_picture_of_two_colours_reduced_to_two() {
        let p = picture(&[[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 255], [255, 255, 255, 255]], 2);
        let mut r = Reducer::new(false);
        let mut bits = [8; 4];
        let (dg, said) = r.color_reduce(&p, 2, &mut bits, None, 0, 0, BOXES, false).unwrap();
        let dg = dg.unwrap();
        assert_eq!(said, "");
        let pal = dg.palette.as_ref().unwrap();
        assert_eq!(pal.count, 2);
        let ix = dg.data.clone().unwrap();
        assert_eq!(ix[0], ix[2]);
        assert_ne!(ix[0], ix[1]);
        assert_eq!(pal.entries[ix[0] as usize].color, [0, 0, 0, 255]);
        assert_eq!(r.count_reduce, Some(vec![[0, 2], [1, 2]]));
        assert_eq!(r.palette_count_effective_entries(Some(&dg)).unwrap(), 2);
        assert_eq!(r.total_couleurs_rgb(&p).unwrap(), 2);
    }

    #[test]
    fn a_reserved_entry_moves_everything_up() {
        let p = picture(&[[10, 10, 10, 255], [200, 200, 200, 255]], 2);
        let mut r = Reducer::new(false);
        let mut bits = [8; 4];
        let (dg, _) = r.color_reduce(&p, 3, &mut bits, None, 0, 1, BOXES, false).unwrap();
        let dg = dg.unwrap();
        let pal = dg.palette.as_ref().unwrap();
        assert_eq!(pal.count, 3);
        assert_eq!(pal.entries[0].color, [0, 0, 0, 255]);
        assert!(dg.data.as_ref().unwrap().iter().all(|&v| v >= 1));
    }

    #[test]
    fn locks_take_the_nearest_entries_and_their_places() {
        let mut pal = DgPalette::allocate(3, &[8; 4]);
        pal.set_entry(0, [1, 1, 1, 255]).unwrap();
        pal.set_entry(1, [250, 250, 250, 255]).unwrap();
        pal.set_entry(2, [100, 0, 0, 255]).unwrap();
        pal.entries[1].flags = 1;
        let e = |c: u8| HistEntry { color: [c, c, c, 255], count: 7, extra: 0 };
        let mut map = vec![e(240), e(20), e(120)];
        assert!(palette_integrate_locks(Some(&mut map), 3, &[8; 4], Some(&pal), 1, 0).unwrap());
        // the entry nearest to the locked colour took it and went to the lock's place, 1
        assert_eq!(map.iter().map(|m| (m.color[0], m.count)).collect::<Vec<_>>(), vec![(20, -1), (250, 1), (120, -1)]);
        let mut few = vec![e(1)];
        assert!(!palette_integrate_locks(Some(&mut few), 1, &[8; 4], Some(&pal), 2, 0).unwrap());
    }

    #[test]
    fn repeated_palette_colours_are_filtered() {
        let mut dg = DgPicture::allocate(2, 1, 3, &[8; 4]);
        let pal = dg.palette.as_mut().unwrap();
        pal.set_entry(0, [5, 5, 5, 255]).unwrap();
        pal.set_entry(1, [9, 9, 9, 255]).unwrap();
        pal.set_entry(2, [5, 5, 5, 255]).unwrap();
        dg.data = Some(vec![2, 1]);
        let mut r = Reducer::new(false);
        r.palette_filter_redundancy(Some(&mut dg)).unwrap();
        let pal = dg.palette.as_ref().unwrap();
        let ix = dg.data.clone().unwrap();
        assert_eq!(pal.entries[ix[0] as usize].color, [5, 5, 5, 255]);
        assert_eq!(pal.entries[ix[1] as usize].color, [9, 9, 9, 255]);
        assert_eq!(pal.entries[2].color, [0, 0, 0, 255]);
        assert_eq!(save_palette(Some(b"x"), Some(b"gif"), Some(&dg)).unwrap(), None);
        assert!(save_palette(Some(b"x"), Some(b"pal"), Some(&dg)).unwrap().unwrap().starts_with(b"1 3\n"));
    }
}
