// SPDX-License-Identifier: MIT
//! The colour-index picture and its palette: the records, the palette's entry operations, the
//! converters to the other picture records, and the two file formats that hold them: the
//! `.dgt` picture and the `.pal` text palette (finding 0014, "Colour-index pictures and
//! palettes").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `dgt_picture.c` (0x085b9c10–0x085bc585). Its
//! `SI_dgColorEquivTable` is [`crate::quant::color_equiv_table`]; its two converters between
//! the plane and the packed record are in `si_picture::PictureRgba`.
//!
//! Three things the original leaves undefined come up all the time and are handled the same
//! way everywhere here:
//! - a palette entry's 8-bit colour word is looked up with the caller's **unclamped** value,
//!   so a value above the channel's depth reads past the table (the standard "alpha 0xff" on
//!   a palette whose alpha is shallower than 8 bits does). The port stores 0 in that byte and
//!   marks it in [`DgEntry::undefined`];
//! - entries added by [`DgPalette::resize`] are whatever the allocator returned; here zero;
//! - 224 bytes of every `.dgt` header are stack memory; here zero.

use crate::effect::Fault;
use crate::quant::color_equiv_table;
use si_picture::{Picture, PictureRgba};
use si_x87::{Round, F80};

/// One palette entry: the original's 20 bytes.
///
/// | Offset | Field |
/// |---|---|
/// | 0 | `color`: red, green, blue, alpha at the palette's depths |
/// | 4 | `wide`: the same colour at 8 bits a channel, red first |
/// | 8, 9 | not touched by any routine read |
/// | 10 | `flags`: bit 0 "locked" |
/// | 11 | not touched |
/// | 12 | `mark`: a 16-bit word; non-zero makes the entry "usable" for the mapping (0x10) |
/// | 14, 15 | not touched |
/// | 16 | `refs`: how many pixels use the entry |
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DgEntry {
    pub color: [u8; 4],
    pub wide: [u8; 4],
    pub spare8: [u8; 2],
    pub flags: u8,
    pub spare11: u8,
    pub mark: u16,
    pub spare14: [u8; 2],
    pub refs: u32,
    /// Not in the original: bit `c` set says byte `c` of `wide` is undefined there (it was
    /// read past the end of a table) and is 0 here.
    pub undefined: u8,
}

/// Flag bit 0 of [`DgEntry::flags`]: the entry is locked.
pub const ENTRY_LOCKED: u8 = 1;

impl DgEntry {
    /// The 20 bytes of the record.
    pub fn to_bytes(&self) -> [u8; 20] {
        let mut b = [0u8; 20];
        b[..4].copy_from_slice(&self.color);
        b[4..8].copy_from_slice(&self.wide);
        b[8..10].copy_from_slice(&self.spare8);
        b[10] = self.flags;
        b[11] = self.spare11;
        b[12..14].copy_from_slice(&self.mark.to_le_bytes());
        b[14..16].copy_from_slice(&self.spare14);
        b[16..20].copy_from_slice(&self.refs.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> DgEntry {
        DgEntry {
            color: [b[0], b[1], b[2], b[3]],
            wide: [b[4], b[5], b[6], b[7]],
            spare8: [b[8], b[9]],
            flags: b[10],
            spare11: b[11],
            mark: u16::from_le_bytes([b[12], b[13]]),
            spare14: [b[14], b[15]],
            refs: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
            undefined: 0,
        }
    }
}

/// The most entries, and the most pixels, the port allocates where the original would ask
/// the C library for whatever the count says.
pub const MAX_ENTRIES: u32 = 1 << 20;
pub const MAX_PIXELS: u32 = 1 << 26;

/// A palette: the original's 0x50-byte `DG_Palette`.
///
/// | Offset | Field |
/// |---|---|
/// | 0 | `count` |
/// | 4 | `bits`: depth of red, green, blue, alpha |
/// | 0x14 | `max`: the largest value of each channel, 2^bits − 1, one byte each |
/// | 0x18 | `size`: bytes of the entries, 20 × count |
/// | 0x1c | pointer to the entries |
/// | 0x20 | never set |
/// | 0x24 | `to8`: four tables from the channel's depth to 8 bits |
/// | 0x34 | `from8`: four tables from 8 bits to the channel's depth |
/// | 0x44, 0x48, 0x4c | `sel`: three entry numbers, kept valid when entries are removed |
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DgPalette {
    /// The count field. It equals `entries.len()` except after an allocation that failed.
    pub count: u32,
    pub bits: [i32; 4],
    pub max: [u8; 4],
    pub size: i32,
    pub entries: Vec<DgEntry>,
    pub to8: [Option<Vec<u8>>; 4],
    pub from8: [Option<Vec<u8>>; 4],
    /// The words at 0x44 (−1 when new), 0x48 (0) and 0x4c (1, or 0 for a palette of one
    /// entry). What the program uses them for was not read.
    pub sel: [i32; 3],
}

fn table(from: i32, to: i32) -> Option<Vec<u8>> {
    // above 16 bits the original asks for up to 2 GB; taken as an allocation that fails
    color_equiv_table(from, to).ok()
}

impl DgPalette {
    /// A palette of `count` black, transparent entries [soft 0x085b9e30 SI_dgPaletteAllocate].
    /// For a count of 0 (or a negative one) there are no entries, but the count field holds
    /// the number all the same.
    pub fn allocate(count: i32, bits: &[i32; 4]) -> DgPalette {
        let mut p = DgPalette { count: count as u32, bits: *bits, size: count.wrapping_mul(20), ..DgPalette::default() };
        for c in 0..4 {
            p.max[c] = (1i32.wrapping_shl(bits[c] as u32)).wrapping_sub(1) as u8;
            p.to8[c] = table(bits[c], 8);
            p.from8[c] = table(8, bits[c]);
        }
        if count > 0 && (count as u32) <= MAX_ENTRIES {
            p.entries = vec![DgEntry::default(); count as usize];
        }
        p.sel = [-1, 0, count.wrapping_sub(1).min(1)];
        p
    }

    /// Sets the colour of entry `i` [soft 0x085b9f28 SI_dgPaletteSetEntry]. Nothing happens
    /// for an entry beyond the count.
    ///
    /// Each value is limited to 0…`max` and stored. The 8-bit word is then looked up in the
    /// `to8` tables with the values **as given**, not as limited: a value outside the table
    /// reads whatever lies beside it in memory (see the module's note).
    pub fn set_entry(&mut self, i: u32, v: [i32; 4]) -> Result<(), Fault> {
        if i >= self.count {
            return Ok(());
        }
        let Some(e) = self.entries.get_mut(i as usize) else { return Err(Fault::NullPlane) };
        e.undefined = 0;
        for c in 0..4 {
            let mut x = v[c];
            if x > self.max[c] as i32 {
                x = self.max[c] as i32;
            }
            if x < 0 {
                x = 0;
            }
            e.color[c] = x as u8;
        }
        for c in 0..4 {
            let t = self.to8[c].as_ref().ok_or(Fault::NullPlane)?;
            match usize::try_from(v[c]).ok().and_then(|k| t.get(k)) {
                Some(&b) => e.wide[c] = b,
                None => {
                    e.wide[c] = 0;
                    e.undefined |= 1 << c;
                }
            }
        }
        Ok(())
    }

    /// Changes the number of entries [soft 0x085ba274 SI_dgPaletteResize].
    ///
    /// The original means to make new entries black and opaque, but calls
    /// [`DgPalette::set_entry`] before it raises the count, so every such call is refused:
    /// **new entries hold whatever the allocator returned**. Here they are zero. A new size of
    /// 0 keeps the old entries' memory and only sets the count.
    pub fn resize(&mut self, n: i32) -> Result<(), Fault> {
        self.size = n.wrapping_mul(20);
        if n < 0 || n as u32 > MAX_ENTRIES {
            return Err(Fault::BadSize);
        }
        if n != 0 {
            self.entries.resize(n as usize, DgEntry::default());
        }
        self.count = n as u32;
        Ok(())
    }

    /// Adds `k` black entries with full alpha at the end
    /// [soft 0x085ba2d0 SI_DG_paletteAddEntries].
    pub fn add_entries(&mut self, k: u32) -> Result<(), Fault> {
        let n = self.count.wrapping_add(k);
        let old = self.entries.get(..self.count as usize).ok_or(Fault::NullPlane)?;
        if n > MAX_ENTRIES {
            return Err(Fault::BadSize);
        }
        let mut new = vec![DgEntry::default(); n as usize];
        new[..old.len()].copy_from_slice(old);
        self.entries = new;
        self.count = n;
        self.size = self.size.wrapping_add(k.wrapping_mul(20) as i32);
        for j in 0..k {
            self.set_entry(self.count - j - 1, [0, 0, 0, 0xff])?;
        }
        Ok(())
    }

    /// Takes entry `i` out [soft 0x085ba394 SI_DG_paletteRemoveEntry]; the three entry
    /// numbers in `sel` that are at or above it move down by one (not below 0). For an `i`
    /// at or beyond the count the original copies past its new array.
    pub fn remove_entry(&mut self, i: u32) -> Result<(), Fault> {
        // the new array has count − 1 entries; the first `i` old ones are copied into it
        // whatever `i` is
        if self.count == 0 && i != 0 {
            return Err(Fault::NullPlane);
        }
        if self.count != 0 && i >= self.count {
            return Err(Fault::BadSize);
        }
        let old = self.entries.get(..self.count as usize).ok_or(Fault::NullPlane)?;
        let new: Vec<DgEntry> = old.iter().enumerate().filter(|(k, _)| *k != i as usize).map(|(_, e)| *e).collect();
        self.entries = new;
        self.count = self.count.wrapping_sub(1);
        self.size = self.size.wrapping_sub(20);
        for k in [1, 2] {
            let v = self.sel[k] as u32;
            if v != 0 && i <= v {
                self.sel[k] = v.wrapping_sub(1) as i32;
            }
        }
        if self.sel[0] > 0 && i <= self.sel[0] as u32 {
            self.sel[0] -= 1;
        }
        Ok(())
    }

    /// Cuts the last `k` entries off [soft 0x085ba47c SI_dgPaletteRemoveEntries]. Returns
    /// false, with nothing changed, unless 0 < `k` < count.
    pub fn remove_entries(&mut self, k: u32) -> bool {
        if k == 0 || k >= self.count || self.entries.len() < self.count as usize {
            return false;
        }
        let n = self.count - k;
        self.entries.truncate(n as usize);
        self.count = n;
        self.size = n.wrapping_mul(20) as i32;
        if n <= self.sel[1] as u32 {
            self.sel[1] = (n - 1) as i32;
        }
        if n <= self.sel[2] as u32 {
            self.sel[2] = 0;
        }
        if self.sel[1] == self.sel[2] && n > 1 {
            self.sel[1] = 0;
            self.sel[2] = 1;
        }
        if n <= self.sel[0] as u32 {
            self.sel[0] = -1;
        }
        true
    }

    /// A copy [soft 0x085ba57c SI_dgPaletteCopy]: a new palette of the same count and depths
    /// (its tables made afresh), the entries and the three entry numbers copied.
    /// [soft 0x085ba60c SI_dgPaletteDispose] is the value going out of scope.
    pub fn copy(&self) -> Result<DgPalette, Fault> {
        let mut p = DgPalette::allocate(self.count as i32, &self.bits);
        let n = self.count as usize;
        if p.entries.len() != n || self.entries.len() < n {
            return Err(Fault::NullPlane);
        }
        p.entries.copy_from_slice(&self.entries[..n]);
        p.sel = self.sel;
        Ok(p)
    }

    /// Brings the palette to other depths [soft 0x085bc38c SI_changePaletteResolution]:
    /// channel by channel new tables, depth and largest value, and every entry's colour
    /// converted through the table from the old depth to the new. **The entries' 8-bit words
    /// are left as they were.** If a table cannot be made the routine stops there, with the
    /// channels before it already changed.
    pub fn change_resolution(&mut self, bits: &[i32; 4]) -> Result<(), Fault> {
        for c in 0..4 {
            let (Some(t), Some(to8), Some(from8)) = (table(self.bits[c], bits[c]), table(bits[c], 8), table(8, bits[c])) else { return Ok(()) };
            self.to8[c] = Some(to8);
            self.from8[c] = Some(from8);
            self.bits[c] = bits[c];
            self.max[c] = (1i32.wrapping_shl(bits[c] as u32)).wrapping_sub(1) as u8;
            if self.entries.len() < self.count as usize {
                return Err(Fault::NullPlane);
            }
            for e in self.entries.iter_mut().take(self.count as usize) {
                // a colour above the old depth's largest value reads past the table
                e.color[c] = *t.get(e.color[c] as usize).ok_or(Fault::ShortPlane)?;
            }
        }
        Ok(())
    }

    /// How many entries are locked (the loop three callers have inline).
    pub fn locked(&self) -> i32 {
        self.entries.iter().take(self.count as usize).filter(|e| e.flags & ENTRY_LOCKED != 0).count() as i32
    }

    fn entry(&self, i: usize) -> Result<&DgEntry, Fault> {
        self.entries.get(i).ok_or(Fault::ShortPlane)
    }

    /// Entry `i`'s colour at 8 bits through the `to8` tables.
    fn wide_of(&self, i: usize) -> Result<[u8; 4], Fault> {
        let e = self.entry(i)?;
        let mut out = [0u8; 4];
        for c in 0..4 {
            out[c] = *self.to8[c].as_ref().ok_or(Fault::NullPlane)?.get(e.color[c] as usize).ok_or(Fault::ShortPlane)?;
        }
        Ok(out)
    }
}

/// A picture of colour indices with its palette: the whole of the original's 0x1c-byte
/// `DG_Picture` (`si_picture::PictureCi` holds the part `picture.c` uses).
///
/// | Offset | Field |
/// |---|---|
/// | 0, 2 | width, height, unsigned 16-bit |
/// | 4 | `size`: 2 × width × height |
/// | 8 | the palette |
/// | 0xc | `data`: one 16-bit index a pixel |
/// | 0x10 | `bytes`: one byte a pixel; no routine read here uses it |
/// | 0x14 | `rect`: four words, 0, height − 1, width − 1, 0 |
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DgPicture {
    pub width: u16,
    pub height: u16,
    pub size: u32,
    pub palette: Option<DgPalette>,
    pub data: Option<Vec<u16>>,
    pub bytes: Option<Vec<u8>>,
    pub rect: [u16; 4],
}

fn pixels(n: i32) -> Option<usize> {
    // SI_calloc returns nothing for a count of 0; a negative or huge count fails in calloc
    (n > 0 && n as u32 <= MAX_PIXELS).then_some(n as usize)
}

impl DgPicture {
    /// An empty picture with a palette of `colors` entries
    /// [soft 0x085b9ca4 SI_dgPictureAllocate]. [soft 0x085b9d34 SI_dgPictureDispose] is the
    /// value going out of scope.
    pub fn allocate(width: i32, height: i32, colors: i32, bits: &[i32; 4]) -> DgPicture {
        let n = height.wrapping_mul(width);
        DgPicture {
            width: width as u16,
            height: height as u16,
            size: height.wrapping_mul(2).wrapping_mul(width) as u32,
            palette: Some(DgPalette::allocate(colors, bits)),
            data: pixels(n).map(|n| vec![0u16; n]),
            bytes: pixels(n).map(|n| vec![0u8; n]),
            rect: [0, (height as u16).wrapping_sub(1), (width as u16).wrapping_sub(1), 0],
        }
    }

    fn area(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Counts again how many pixels use each palette entry
    /// [soft 0x085b9dc0 SI_dgPictureUpdateRefCount]. Nothing happens without a palette or
    /// without entries.
    pub fn update_ref_count(&mut self) -> Result<(), Fault> {
        let n = self.area();
        let Some(pal) = self.palette.as_mut() else { return Ok(()) };
        if pal.entries.is_empty() {
            return Ok(());
        }
        if pal.entries.len() < pal.count as usize {
            return Err(Fault::ShortPlane);
        }
        for e in pal.entries.iter_mut().take(pal.count as usize) {
            e.refs = 0;
        }
        if n == 0 {
            return Ok(());
        }
        let data = self.data.as_ref().ok_or(Fault::NullPlane)?.get(..n).ok_or(Fault::ShortPlane)?;
        for &ix in data {
            let e = pal.entries.get_mut(ix as usize).ok_or(Fault::ShortPlane)?;
            e.refs = e.refs.wrapping_add(1);
        }
        Ok(())
    }

    /// The picture at another size [soft 0x085b9ff4 SI_dgScalePic], nearest pixel, with a
    /// copy of the palette. `None` for no picture or a failed allocation.
    ///
    /// - The new size is |`w`| × |`h`|, each at least 1. A negative `h` turns the picture
    ///   upside down; a `w` **below 1** mirrors it.
    /// - Asked for its own size, the pixels are copied and the palette's counts are left.
    /// - Otherwise output row y takes source row trunc(height × (y / (new height + 1))) and
    ///   output column x source column trunc((x / (new width + 1)) × width): note the **+ 1**
    ///   (the icon makers of `geticon.c` have the same), so even a small change of size
    ///   resamples the whole picture. Then the palette's counts are made again.
    /// - The result has no byte buffer and a zero rectangle.
    pub fn scale(&self, w: i32, h: i32) -> Result<Option<DgPicture>, Fault> {
        let aw = w.wrapping_abs().max(1);
        let ah = h.wrapping_abs().max(1);
        let Some(n) = pixels(aw.wrapping_mul(ah)) else { return Ok(None) };
        let mut out = DgPicture { width: aw as u16, height: ah as u16, size: ah.wrapping_mul(2).wrapping_mul(aw) as u32, data: Some(vec![0u16; n]), ..DgPicture::default() };
        out.palette = Some(self.palette.as_ref().ok_or(Fault::NullPlane)?.copy()?);
        let src = self.data.as_ref().ok_or(Fault::NullPlane)?;
        let dst = out.data.as_mut().unwrap();
        if self.width as i32 == w && self.height as i32 == h {
            dst.copy_from_slice(src.get(..n).ok_or(Fault::ShortPlane)?);
            return Ok(Some(out));
        }
        let (ow, oh) = (out.width as usize, out.height as usize);
        if ow * oh > n {
            return Err(Fault::BadSize);
        }
        for y in 0..oh {
            let mut row = (F80::from_i32(self.height as i32) * (F80::from_i32(y as i32) / F80::from_i32(oh as i32 + 1))).to_i32(Round::Zero);
            if h < 0 {
                row = self.height as i32 - row - 1;
            }
            for x in 0..ow {
                let col = ((F80::from_i32(x as i32) / F80::from_i32(ow as i32 + 1)) * F80::from_i32(self.width as i32)).to_i32(Round::Zero);
                let at = (self.width as i32).wrapping_mul(row).wrapping_add(col);
                let v = *usize::try_from(at).ok().and_then(|k| src.get(k)).ok_or(Fault::ShortPlane)?;
                let to = if w < 1 { ow * (y + 1) - 1 - x } else { ow * y + x };
                dst[to] = v;
            }
        }
        out.update_ref_count()?;
        Ok(Some(out))
    }

    /// The picture as planes [soft 0x085bbd8c SI_convertDG_Pict2SIPicture]: every index
    /// replaced by its entry's colour at 8 bits (through the `to8` tables, not the entry's
    /// own 8-bit word), **rows turned upside down**. Ratio 1.0, the spare word 1, and
    /// `fields` **0xe0** (the channel mask of red, green and blue, in the wrong field).
    /// An empty picture gives `None`; the original leaves the header set and four null planes.
    pub fn to_picture(&self) -> Result<Option<Picture>, Fault> {
        let (w, h) = (self.width as usize, self.height as usize);
        if w * h == 0 {
            return Ok(None);
        }
        let pal = self.palette.as_ref().ok_or(Fault::NullPlane)?;
        let data = self.data.as_ref().ok_or(Fault::NullPlane)?.get(..w * h).ok_or(Fault::ShortPlane)?;
        let mut planes = [vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h]];
        let mut k = 0;
        for y in (0..h).rev() {
            for x in 0..w {
                let c = pal.wide_of(data[w * y + x] as usize)?;
                for (plane, v) in planes.iter_mut().zip(c) {
                    plane[k] = v;
                }
                k += 1;
            }
        }
        let [r, g, b, a] = planes;
        Ok(Some(Picture { width: self.width as i16, height: self.height as i16, ratio: 1.0, fields: 0xe0, pad: 1, planes: [Some(r), Some(g), Some(b), Some(a)] }))
    }

    /// The picture packed [soft 0x085bc2bc SI_convertDG_Pict2RGBA]: every index replaced by
    /// its entry's 8-bit word, rows turned upside down, the header as in
    /// [`DgPicture::to_picture`]. The second value is false when a byte copied is one the
    /// original leaves undefined (see [`DgEntry::undefined`]).
    pub fn to_rgba(&self) -> Result<Option<(PictureRgba, bool)>, Fault> {
        let (w, h) = (self.width as usize, self.height as usize);
        if w * h == 0 || (w * h) as u32 > MAX_PIXELS {
            return Ok(None);
        }
        let pal = self.palette.as_ref().ok_or(Fault::NullPlane)?;
        let data = self.data.as_ref().ok_or(Fault::NullPlane)?.get(..w * h).ok_or(Fault::ShortPlane)?;
        let mut out = Vec::with_capacity(4 * w * h);
        let mut defined = true;
        for y in (0..h).rev() {
            for x in 0..w {
                let e = pal.entry(data[w * y + x] as usize)?;
                out.extend_from_slice(&e.wide);
                defined &= e.undefined == 0;
            }
        }
        Ok(Some((PictureRgba { width: self.width as i16, height: self.height as i16, ratio: 1.0, fields: 0xe0, pad: 1, data: out }, defined)))
    }
}

/// A file name with an extension [soft 0x085ba6bc SI_addExtensionToFilename]: the name as it
/// is when the text after its last dot **starts with** the extension, else name, dot,
/// extension. Nothing for an empty extension.
pub fn add_extension_to_filename(name: Option<&[u8]>, ext: Option<&[u8]>) -> Option<Vec<u8>> {
    let (name, ext) = (name?, ext?);
    if ext.is_empty() {
        return None;
    }
    if let Some(dot) = name.iter().rposition(|&b| b == b'.') {
        if name[dot + 1..].starts_with(ext) {
            return Some(name.to_vec());
        }
    }
    Some([name, b".", ext].concat())
}

// ---------------------------------------------------------------- .dgt files

/// The text both writers put at offset 16.
pub const DGT_NOTE: &[u8; 16] = b"Col. Reduction  ";
/// Bytes 32 to 255 of a `.dgt` file: stack memory in the original, zero here.
pub const DGT_STALE: std::ops::Range<usize> = 0x20..0x100;

/// The palette part of a `.dgt` file: one record of two 16-bit words per entry (its number,
/// its colour), then records with colour 0 up to a multiple of sixteen.
///
/// A colour is 15 bits: each channel brought to 5 bits as trunc(value × float(31 / max) +
/// 0.5), its upper four bits at bit 0 (red), 4 (green) and 8 (blue), its lowest bit at bit
/// 12, 13 and 14.
fn dgt_palette(out: &mut Vec<u8>, pal: &DgPalette, pad: u32, padded: i16) -> Result<(), Fault> {
    let scale: Vec<f32> = (0..3).map(|c| (F80::from_f64(31.0) / F80::from_i32(1i32.wrapping_shl(pal.bits[c] as u32).wrapping_sub(1))).to_f32()).collect();
    if pal.entries.len() < pal.count as usize {
        return Err(Fault::NullPlane);
    }
    for (i, e) in pal.entries.iter().take(pal.count as usize).enumerate() {
        out.extend_from_slice(&(i as u16).to_le_bytes());
        let five = |c: usize| (F80::from_i32(e.color[c] as i32) * F80::from_f32(scale[c]) + F80::from_f64(0.5)).to_i32(Round::Zero) as u8 as u32;
        let (b, g, r) = (five(2), five(1), five(0));
        let word = (b & 1) << 14 | (b & 0x1e) << 7 | (g & 1) << 13 | (g & 0x1e) << 3 | (r & 1) << 12 | (r >> 1) & 0xf;
        out.extend_from_slice(&(word as u16).to_le_bytes());
    }
    for j in 0..pad {
        out.extend_from_slice(&(padded.wrapping_sub(pad as i16).wrapping_add(j as i16)).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
    }
    Ok(())
}

fn dgt_head(pal: &DgPalette, pixel_bytes: u32, size: [u16; 2]) -> (Vec<u8>, u32, i16) {
    let low = pal.count as u16;
    let pad = (0x10 - (low & 0xf) as u32) & 0xf;
    let padded = low.wrapping_add(pad as u16) as i16;
    let table = (padded as i32).wrapping_mul(4);
    let mut out = Vec::new();
    out.extend_from_slice(&0x100u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(table.wrapping_add(0x120) as u32).wrapping_add(pixel_bytes).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(DGT_NOTE);
    out.extend_from_slice(&[0u8; 0xe0]); // DGT_STALE
    out.extend_from_slice(&(table.wrapping_add(0x20) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&size[0].to_le_bytes());
    out.extend_from_slice(&size[1].to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&[b' '; 16]);
    (out, pad, padded)
}

/// A `.dgt` file holding the picture's palette and no pixels
/// [soft 0x085bac88 SI_writeDGTpalette]: the layout of [`write_dgt_file`] with a size of
/// 0 × 0. The bytes are what the original hands to `fwrite` on this (little-endian) build.
pub fn write_dgt_palette(p: &DgPicture) -> Result<Vec<u8>, Fault> {
    let pal = p.palette.as_ref().ok_or(Fault::NullPlane)?;
    let (mut out, pad, padded) = dgt_head(pal, 0, [0, 0]);
    dgt_palette(&mut out, pal, pad, padded)?;
    Ok(out)
}

/// A `.dgt` file [soft 0x085bb1a4 SI_writeDGTfile].
///
/// | Offset | Size | Field |
/// |---|---|---|
/// | 0 | 4 | 0x100: where the first picture starts |
/// | 4 | 4 | 1: the number of pictures |
/// | 8 | 4 | the file's length: 0x120 + 4 × palette records + 2 × width × height |
/// | 12 | 4 | 0 |
/// | 16 | 16 | `Col. Reduction  ` |
/// | 32 | 224 | not set (stack memory) |
/// | 0x100 | 4 | 0x20 + 4 × palette records: from here to the pixels |
/// | 0x104 | 4 | 0 |
/// | 0x108 | 2, 2 | width, height |
/// | 0x10c | 4 | 0 |
/// | 0x110 | 16 | blanks |
/// | 0x120 | 4 × n | the palette, n a multiple of 16: see [`write_dgt_palette`] |
/// | … | 2 × w × h | the indices, **last row in memory first** |
///
/// Every field is written in the machine's byte order: little-endian here.
pub fn write_dgt_file(p: &DgPicture) -> Result<Vec<u8>, Fault> {
    let pal = p.palette.as_ref().ok_or(Fault::NullPlane)?;
    let (w, h) = (p.width as usize, p.height as usize);
    let (mut out, pad, padded) = dgt_head(pal, (w as u32).wrapping_mul(h as u32).wrapping_mul(2), [p.width, p.height]);
    dgt_palette(&mut out, pal, pad, padded)?;
    if w * h != 0 {
        let data = p.data.as_ref().ok_or(Fault::NullPlane)?.get(..w * h).ok_or(Fault::ShortPlane)?;
        for y in (0..h).rev() {
            for x in 0..w {
                out.extend_from_slice(&data[w * y + x].to_le_bytes());
            }
        }
    }
    Ok(out)
}

/// A variable the reader fills with `fread` and never tests: a read that comes up short
/// leaves it (or its upper bytes) as it was, and it may never have been set at all.
#[derive(Clone, Copy)]
struct Cell<const N: usize> {
    b: [u8; N],
    set: bool,
}

impl<const N: usize> Cell<N> {
    const UNSET: Cell<N> = Cell { b: [0; N], set: false };

    fn get(&self) -> Result<[u8; N], Fault> {
        if self.set {
            Ok(self.b)
        } else {
            Err(Fault::Uninitialised)
        }
    }
}

struct Rd<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Rd<'_> {
    /// `fread(&cell, N, 1, f)`: as many bytes as the file still has.
    fn read<const N: usize>(&mut self, c: &mut Cell<N>) {
        let have = self.data.len().saturating_sub(self.pos).min(N);
        if have > 0 {
            c.b[..have].copy_from_slice(&self.data[self.pos..self.pos + have]);
            self.pos += have;
        }
        if have == N {
            c.set = true;
        }
    }

    /// `fseek`: a position before the start is refused and nothing moves.
    fn seek(&mut self, to: i64) {
        if (0..=i32::MAX as i64).contains(&to) {
            self.pos = to as usize;
        }
    }

    fn skip(&mut self, by: i64) {
        self.seek(self.pos as i64 + by);
    }
}

/// Picture number `number` (from 1) of a `.dgt` file [soft 0x085ba838 SI_readDGTfile], or
/// `None` when the file does not open (`data` is `None`).
///
/// - The file's first word is where the first picture starts; nothing else of the header is
///   looked at. To reach a later picture the reader walks over the ones before it by their
///   block sizes and pixel counts.
/// - The palette has (block size − 0x20) / 4 entries (a signed 16-bit number) at **5 bits a
///   channel**; each record's colour is unpacked as [`write_dgt_palette`] packs it, alpha
///   0xff (limited to 31; its 8-bit word is undefined, see the module's note). The record's
///   own number may be anything below 32,768: pixels name colours by that number, through a
///   table of 65,536 words that the reader indexes with the number **as a signed 16-bit
///   value**. A number of 0x8000 or more, in a record or in a pixel, is written or read up
///   to 128 KB in front of that table ([`Fault::ShortPlane`] here).
/// - Pixels are stored in file order: **the reader does not turn the rows** that the writer
///   wrote last row first. Each pixel adds one to its entry's count.
/// - Nothing is tested. A file that ends early leaves the variable being read as it was, so
///   the rest of the palette repeats the last record and the rest of the picture the last
///   index. A file too short to set a variable at all makes the original use stack memory:
///   [`Fault::Uninitialised`]. A name without a `/` makes it copy a string from address 1.
/// - The port refuses a picture of more than a million pixels that the file does not hold
///   in full, and any of more than [`MAX_PIXELS`]; the original would ask for the memory
///   whatever the file holds.
pub fn read_dgt_file(name: &[u8], data: Option<&[u8]>, number: i32) -> Result<Option<DgPicture>, Fault> {
    if !name.contains(&b'/') {
        return Err(Fault::NullPlane);
    }
    let Some(data) = data else { return Ok(None) };
    let mut f = Rd { data, pos: 0 };
    let mut first = Cell::<4>::UNSET;
    let mut word = Cell::<4> { b: [0; 4], set: true };
    let mut unused4 = Cell::<4>::UNSET;
    let mut unused16 = Cell::<16>::UNSET;
    let mut unused2 = Cell::<2>::UNSET;
    let (mut size, mut w, mut h) = (Cell::<4>::UNSET, Cell::<2>::UNSET, Cell::<2>::UNSET);
    f.read(&mut first);
    f.read(&mut unused4);
    f.read(&mut unused4);
    f.read(&mut word);
    f.read(&mut unused16);
    f.seek(i32::from_le_bytes(first.get()?) as i64);
    let records = |size: &Cell<4>| -> Result<i16, Fault> { Ok((u32::from_le_bytes(size.get()?).wrapping_sub(0x20) >> 2) as i16) };
    for _ in 0..number.wrapping_sub(1).max(0) {
        f.read(&mut size);
        f.skip(4);
        f.read(&mut w);
        f.read(&mut h);
        f.skip(0x14);
        f.skip((records(&size)? as i64) << 2);
        f.skip((u16::from_le_bytes(w.get()?) as u32).wrapping_mul(u16::from_le_bytes(h.get()?) as u32).wrapping_mul(2) as i32 as i64);
    }
    f.read(&mut size);
    let colors = records(&size)?;
    f.read(&mut unused2);
    f.read(&mut unused2);
    f.read(&mut w);
    f.read(&mut h);
    f.read(&mut word);
    f.read(&mut unused16);
    let (width, height) = (u16::from_le_bytes(w.get()?), u16::from_le_bytes(h.get()?));
    // Before allocating: a picture of more than a million pixels must be in the file. (The
    // original allocates whatever the header says and fills what the file does not hold
    // with the last number read; the port does the same up to that size.)
    let pixels = width as u32 * height as u32;
    let left = data.len().saturating_sub(f.pos);
    if pixels > MAX_PIXELS || (pixels > 1 << 20 && pixels as usize * 2 > left) {
        return Err(Fault::BadSize);
    }
    let mut pic = DgPicture::allocate(width as i32, height as i32, colors as i32, &[5; 4]);
    let mut remap = vec![0u32; 0x10000]; // 0x40000 bytes; only the lower half can be reached
    let (mut index, mut color) = (Cell::<2>::UNSET, Cell::<2>::UNSET);
    let pal = pic.palette.as_mut().unwrap();
    for i in 0..colors.max(0) as u32 {
        f.read(&mut index);
        f.read(&mut color);
        // the two halves share one 32-bit variable; the colour is its low half
        let v = u16::from_le_bytes(color.get()?) as i32;
        let number = i16::from_le_bytes(index.get()?);
        pal.set_entry(i, [(v & 0xf) * 2 | (v >> 12) & 1, (v & 0xf0) >> 3 | (v & 0x2000) >> 13, (v & 0xf00) >> 7 | (v & 0x4000) >> 14, 0xff])?;
        // the number is taken as signed: one of 0x8000 or more lands in front of the table
        *usize::try_from(number).ok().and_then(|k| remap.get_mut(k)).ok_or(Fault::ShortPlane)? = i;
    }
    if let Some(px) = pic.data.as_mut() {
        for p in px.iter_mut() {
            f.read(&mut index);
            let number = i16::from_le_bytes(index.get()?);
            let ix = *usize::try_from(number).ok().and_then(|k| remap.get(k)).ok_or(Fault::ShortPlane)? as u16;
            *p = ix;
            let e = pal.entries.get_mut(ix as usize).ok_or(Fault::NullPlane)?;
            e.refs = e.refs.wrapping_add(1);
        }
    }
    Ok(Some(pic))
}

// ---------------------------------------------------------------- .pal files

/// The grid a palette of `count` entries is announced with in a `.pal` file: 1 × count up
/// to ten entries, 10 × count / 10 when that divides, else the smallest divisor from 11 up
/// with its quotient, else 1 × count.
pub fn pal_grid(count: u32) -> (u32, u32) {
    if count > 10 {
        if count % 10 == 0 {
            return (10, count / 10);
        }
        let mut d = 11;
        while d < count {
            if count % d == 0 {
                return (d, count / d);
            }
            d += 1;
        }
    }
    (1, count)
}

/// A palette as a text file [soft 0x085bbbec SI_userPalWriteFile]:
///
/// ```text
/// <columns> <rows>
/// 1.0
/// 0.0 0.0
/// 0
///
/// <red> <green> <blue> <alpha>        one line an entry, each value 8-bit colour / 255, "%f"
/// ```
///
/// The 8-bit colour comes from the `to8` tables, not from the entry's 8-bit word. What the
/// three lines after the first mean was not found in the code: the reader skips them.
pub fn write_user_pal(pal: &DgPalette) -> Result<Vec<u8>, Fault> {
    let (columns, rows) = pal_grid(pal.count);
    let mut out = format!("{} {}\n1.0\n0.0 0.0\n0\n\n", columns as i32, rows as i32).into_bytes();
    if pal.entries.len() < pal.count as usize {
        return Err(Fault::NullPlane);
    }
    for i in 0..pal.count as usize {
        let c = pal.wide_of(i)?.map(|v| (F80::from_i32(v as i32) / F80::from_f64(255.0)).to_f64());
        out.extend_from_slice(format!("{:.6} {:.6} {:.6} {:.6}\n", c[0], c[1], c[2], c[3]).as_bytes());
    }
    Ok(out)
}

/// The C library's text scanning, as far as the `.pal` reader needs it. Not Softimage code.
struct Text<'a> {
    data: &'a [u8],
    pos: usize,
    /// The stream's end-of-file mark: set when a read meets the end.
    eof: bool,
}

impl Text<'_> {
    fn peek(&mut self) -> Option<u8> {
        let b = self.data.get(self.pos).copied();
        if b.is_none() {
            self.eof = true;
        }
        b
    }

    fn skip_blanks(&mut self) {
        while let Some(b) = self.peek() {
            if !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
                break;
            }
            self.pos += 1;
        }
    }

    /// The loop the reader has after every `fscanf`: characters up to and including the
    /// next line feed, or to the end of the file.
    fn skip_line(&mut self) {
        while !self.eof {
            match self.peek() {
                Some(b) => {
                    self.pos += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    /// `%d`: decimal digits with an optional sign.
    fn int(&mut self) -> Option<i32> {
        self.skip_blanks();
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let digits = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == digits {
            return None;
        }
        let mut v: i64 = 0;
        for &b in &self.data[digits..self.pos] {
            v = (v * 10 + (b - b'0') as i64).min(1 << 40);
        }
        if self.data[start] == b'-' {
            v = -v;
        }
        // strtol's limits, then the store into an int
        Some(v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }

    /// The characters `%f` gathers: sign, digits, a point, digits, and an exponent. A number
    /// in another notation the C library also takes (hexadecimal, `inf`, `nan`) is not
    /// modelled: it reads as no number here.
    fn float_text(&mut self) -> Option<&[u8]> {
        self.skip_blanks();
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = 0;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        let mut end = self.pos;
        if matches!(self.peek(), Some(b'e' | b'E')) {
            // the letter is used up even when no exponent follows it
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let at = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos > at {
                end = self.pos;
            }
        }
        Some(&self.data[start..end])
    }

    /// `%f` into a float: the text rounded once to single precision.
    fn float(&mut self) -> Option<f32> {
        let t = self.float_text()?;
        std::str::from_utf8(t).ok()?.trim_start_matches('+').parse().ok()
    }
}

/// `atof`: the longest number at the start of the text, after blanks; 0.0 for none. Decimal
/// notation only (see [`Text::float_text`]).
fn atof(text: &[u8]) -> f64 {
    let mut t = Text { data: text, pos: 0, eof: false };
    match t.float_text() {
        Some(s) => std::str::from_utf8(s).ok().and_then(|s| s.trim_start_matches('+').parse().ok()).unwrap_or(0.0),
        None => 0.0,
    }
}

/// A palette from a text file [soft 0x085bb710 SI_userPalReadFile], 8 bits a channel, or
/// `None` when the file does not open.
///
/// - The first line holds two whole numbers; their product is the number of entries. The
///   next three lines are scanned (`%f`, `%f %f`, `%d`) and dropped, and one more line is
///   skipped.
/// - Each entry is three numbers, limited to 0…1. What is left of its line, if anything, is
///   the alpha (`atof`, limited to 0…1), otherwise alpha is 1: so **a line that ends with a
///   blank after the third number has alpha 0**. A value v becomes trunc(255 × v + 0.5).
/// - Nothing is tested. A number that does not scan leaves its variable as it was: an entry
///   that fails to scan repeats the one before it, and after the end of the file the rest of
///   the palette repeats the last entry, with the alpha of the last line that had one.
///   Where that makes the original use a variable nobody set, this returns
///   [`Fault::Uninitialised`].
/// - A value that is not a number is stored as given and makes the entry's lookup read far
///   outside its table: [`Fault::ShortPlane`] here.
pub fn read_user_pal(data: Option<&[u8]>) -> Result<Option<DgPalette>, Fault> {
    let Some(data) = data else { return Ok(None) };
    let mut f = Text { data, pos: 0, eof: false };
    let columns = f.int();
    let rows = if columns.is_some() { f.int() } else { None };
    f.skip_line();
    let count = columns.ok_or(Fault::Uninitialised)?.wrapping_mul(rows.ok_or(Fault::Uninitialised)?);
    // before allocating: a count beyond 65,536 must at least be no larger than the file
    // (the original allocates what the first line says and repeats the last entry)
    if count > 0 && (count as u32 > MAX_ENTRIES || (count > 1 << 16 && count as usize > data.len())) {
        return Err(Fault::BadSize);
    }
    let mut pal = DgPalette::allocate(count, &[8; 4]);
    // three floats share the stack with the header's scans: the first takes "%f", then the
    // first of "%f %f", then the bits of "%d"; the second takes the second of "%f %f"
    let (mut r, mut g, mut b): (Option<f32>, Option<f32>, Option<f32>) = (None, None, None);
    if let Some(v) = f.float() {
        r = Some(v);
    }
    f.skip_line();
    if let Some(v) = f.float() {
        r = Some(v);
        if let Some(v) = f.float() {
            g = Some(v);
        }
    }
    f.skip_line();
    if let Some(v) = f.int() {
        r = Some(f32::from_bits(v as u32));
    }
    f.skip_line();
    f.skip_line();
    let mut rest: Option<Vec<u8>> = None;
    let (zero, one) = (F80::ZERO, F80::from_f32(1.0));
    for i in 0..count.max(0) as u32 {
        if let Some(v) = f.float() {
            r = Some(v);
            if let Some(v) = f.float() {
                g = Some(v);
                if let Some(v) = f.float() {
                    b = Some(v);
                }
            }
        }
        let limit = |v: Option<f32>| -> Result<f32, Fault> {
            let v = v.ok_or(Fault::Uninitialised)?;
            let x = F80::from_f32(v);
            Ok(if x.lt(zero) {
                0.0
            } else if x.gt(one) {
                1.0
            } else {
                v
            })
        };
        let (rv, gv, bv) = (limit(r)?, limit(g)?, limit(b)?);
        (r, g, b) = (Some(rv), Some(gv), Some(bv));
        // "%[^\n]": what is left of the line, if it is not empty
        let start = f.pos;
        while matches!(f.peek(), Some(c) if c != b'\n') {
            f.pos += 1;
        }
        let matched = if f.pos > start {
            if f.pos - start > 1023 {
                return Err(Fault::BadSize); // the line buffer is 1,024 bytes
            }
            rest = Some(data[start..f.pos].to_vec());
            true
        } else {
            // nothing before the line feed: 0; nothing at all: the end-of-file answer, which
            // the routine takes for "matched"
            f.pos >= data.len()
        };
        let alpha = if matched {
            let a = atof(rest.as_ref().ok_or(Fault::Uninitialised)?) as f32;
            let x = F80::from_f32(a);
            if zero.gt(x) {
                0.0
            } else if one.lt(x) {
                1.0
            } else {
                a
            }
        } else {
            1.0
        };
        let byte = |v: f32| (F80::from_f64(255.0) * F80::from_f32(v) + F80::from_f64(0.5)).to_i32(Round::Zero);
        let v = [byte(rv), byte(gv), byte(bv), byte(alpha)];
        if v.iter().any(|x| !(0..=255).contains(x)) {
            return Err(Fault::ShortPlane);
        }
        pal.set_entry(i, v)?;
        f.skip_line();
    }
    Ok(Some(pal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_palette_and_its_tables() {
        let p = DgPalette::allocate(4, &[5, 6, 5, 8]);
        assert_eq!((p.count, p.size, p.max, p.sel), (4, 80, [31, 63, 31, 255], [-1, 0, 1]));
        assert_eq!(p.to8[0].as_ref().unwrap().len(), 32);
        assert_eq!(p.from8[1].as_ref().unwrap()[255], 63);
        assert_eq!(DgPalette::allocate(1, &[8; 4]).sel, [-1, 0, 0]);
        assert_eq!(DgPalette::allocate(0, &[8; 4]).entries.len(), 0);
    }

    #[test]
    fn an_entry_is_limited_but_looked_up_as_given() {
        let mut p = DgPalette::allocate(2, &[5, 5, 5, 5]);
        p.set_entry(0, [31, 0, 7, 0xff]).unwrap();
        let e = p.entries[0];
        assert_eq!(e.color, [31, 0, 7, 31]);
        assert_eq!((e.wide[0], e.wide[1], e.undefined), (255, 0, 8)); // alpha read past the table
        p.set_entry(5, [1, 1, 1, 1]).unwrap(); // beyond the count: nothing
        let mut q = DgPalette::allocate(1, &[8; 4]);
        q.set_entry(0, [300, -4, 9, 255]).unwrap();
        assert_eq!((q.entries[0].color, q.entries[0].undefined), ([255, 0, 9, 255], 3));
    }

    #[test]
    fn growing_a_palette_does_not_colour_the_new_entries() {
        let mut p = DgPalette::allocate(1, &[8; 4]);
        p.set_entry(0, [1, 2, 3, 4]).unwrap();
        p.resize(3).unwrap();
        assert_eq!((p.count, p.size, p.entries[2].color), (3, 60, [0, 0, 0, 0]));
        p.add_entries(2).unwrap();
        assert_eq!((p.count, p.entries[4].color, p.entries[4].wide), (5, [0, 0, 0, 255], [0, 0, 0, 255]));
        p.sel = [4, 4, 2];
        p.remove_entry(2).unwrap();
        assert_eq!((p.count, p.sel), (4, [3, 3, 1]));
        assert!(p.remove_entries(3));
        assert_eq!((p.count, p.sel), (1, [-1, 0, 0]));
        assert!(!p.remove_entries(1));
    }

    #[test]
    fn a_dgt_file_keeps_palette_and_pixels_and_comes_back_upside_down() {
        let mut p = DgPicture::allocate(2, 2, 3, &[5; 4]);
        let pal = p.palette.as_mut().unwrap();
        pal.set_entry(0, [31, 0, 0, 31]).unwrap();
        pal.set_entry(1, [0, 17, 0, 31]).unwrap();
        pal.set_entry(2, [1, 2, 30, 31]).unwrap();
        p.data = Some(vec![0, 1, 2, 2]);
        let file = write_dgt_file(&p).unwrap();
        assert_eq!(file.len(), 0x120 + 16 * 4 + 8);
        assert_eq!(&file[16..32], DGT_NOTE);
        let back = read_dgt_file(b"/x/a.dgt", Some(&file), 1).unwrap().unwrap();
        assert_eq!((back.width, back.height), (2, 2));
        assert_eq!(back.data, Some(vec![2, 2, 0, 1]));
        let bp = back.palette.unwrap();
        assert_eq!(bp.count, 16);
        assert_eq!((bp.entries[0].color, bp.entries[1].color, bp.entries[2].color), ([31, 0, 0, 31], [0, 17, 0, 31], [1, 2, 30, 31]));
        assert_eq!((bp.entries[0].refs, bp.entries[2].refs), (1, 2));
        assert_eq!(read_dgt_file(b"a.dgt", Some(&file), 1), Err(Fault::NullPlane));
        assert_eq!(read_dgt_file(b"/a.dgt", Some(&file[..3]), 1), Err(Fault::Uninitialised));
        assert_eq!(write_dgt_palette(&p).unwrap().len(), 0x120 + 64);
    }

    #[test]
    fn a_text_palette_round_trip_and_the_blank_that_makes_alpha_zero() {
        let mut p = DgPalette::allocate(12, &[8; 4]);
        for i in 0..12 {
            p.set_entry(i, [i as i32 * 20, 255, 0, 128]).unwrap();
        }
        let text = write_user_pal(&p).unwrap();
        assert!(text.starts_with(b"1 12\n1.0\n0.0 0.0\n0\n\n0.000000 1.000000 0.000000 0.501961\n"));
        let back = read_user_pal(Some(&text)).unwrap().unwrap();
        assert_eq!(back.entries.iter().map(|e| e.color).collect::<Vec<_>>(), p.entries.iter().map(|e| e.color).collect::<Vec<_>>());
        let q = read_user_pal(Some(b"1 3\n1.0\n0.0 0.0\n0\n\n1 0 0\n0 1 0 \n2 -1 0.5 0.25\n")).unwrap().unwrap();
        assert_eq!(q.entries.iter().map(|e| e.color).collect::<Vec<_>>(), vec![[255, 0, 0, 255], [0, 255, 0, 0], [255, 0, 128, 64]]);
        assert_eq!(pal_grid(22), (11, 2));
        assert_eq!(pal_grid(13), (1, 13));
        assert_eq!(pal_grid(30), (10, 3));
    }

    #[test]
    fn damaged_files_are_refused_or_read_but_never_panic() {
        let mut seed = 7u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let mut p = DgPicture::allocate(5, 3, 20, &[5; 4]);
        p.data = Some((0..15).map(|i| i % 20).collect());
        let good = write_dgt_file(&p).unwrap();
        for cut in 0..good.len() {
            let _ = read_dgt_file(b"/a", Some(&good[..cut]), 1);
            let _ = read_dgt_file(b"/a", Some(&good[..cut]), 2);
        }
        for _ in 0..4000 {
            let mut bad = good.clone();
            for _ in 0..1 + noise() % 4 {
                let at = (noise() as usize * 256 + noise() as usize) % bad.len();
                bad[at] = noise();
            }
            let _ = read_dgt_file(b"/a", Some(&bad), 1 + (noise() % 3) as i32);
        }
        // a size the header claims and the file does not hold
        let mut huge = good.clone();
        huge[0x108..0x10c].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        assert_eq!(read_dgt_file(b"/a", Some(&huge), 1), Err(Fault::BadSize));
        let text = b"3 2\n1.0\n0.0 0.0\n0\n\n0 0 0\n1 1 1 1\n0.5 0.5 0.5\n0.1 0.2 0.3 0.4\n1 0 1\n0 1 0\n".to_vec();
        for cut in 0..text.len() {
            let _ = read_user_pal(Some(&text[..cut]));
        }
        for _ in 0..4000 {
            let mut bad = text.clone();
            let at = noise() as usize % bad.len();
            bad[at] = [b' ', b'\n', b'9', b'-', b'e', b'.', b'x', 0][noise() as usize % 8];
            let _ = read_user_pal(Some(&bad));
        }
        assert_eq!(read_user_pal(Some(b"99999 99999\n")), Err(Fault::BadSize));
    }

    #[test]
    fn scaling_and_the_converters() {
        let mut p = DgPicture::allocate(2, 2, 2, &[8; 4]);
        let pal = p.palette.as_mut().unwrap();
        pal.set_entry(0, [10, 20, 30, 40]).unwrap();
        pal.set_entry(1, [200, 201, 202, 203]).unwrap();
        p.data = Some(vec![0, 1, 1, 1]);
        let same = p.scale(2, 2).unwrap().unwrap();
        assert_eq!(same.data, p.data);
        let wide = p.scale(4, 2).unwrap().unwrap();
        assert_eq!(wide.data, Some(vec![0, 0, 0, 1, 0, 0, 0, 1])); // row 1 samples source row 0: 2 × (1 / 3)
        assert_eq!(wide.palette.as_ref().unwrap().entries[0].refs, 6);
        let flat = p.to_picture().unwrap().unwrap();
        assert_eq!((flat.fields, flat.pad), (0xe0, 1));
        assert_eq!(flat.planes[0], Some(vec![200, 200, 10, 200]));
        let (packed, defined) = p.to_rgba().unwrap().unwrap();
        assert!(defined);
        assert_eq!(&packed.data[8..12], &[10, 20, 30, 40]);
        assert_eq!(add_extension_to_filename(Some(b"a.b/c"), Some(b"pal")), Some(b"a.b/c.pal".to_vec()));
        assert_eq!(add_extension_to_filename(Some(b"c.palette"), Some(b"pal")), Some(b"c.palette".to_vec()));
    }
}
