// SPDX-License-Identifier: MIT
//! The display's texture loader and its cache: the last four routines of `picture.c`
//! (finding 0014, "The texture loader").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. They are in this crate, not in `si-picture`, because they need
//! the tiling of `pic_tile.c`.
//!
//! Two records of other subsystems are involved, the texture map (`__SI_LclMap`) and a
//! window's graphic properties (`SI_GraphicProps`). Neither is established as a typed record
//! yet, so the map is held as a memory image with every offset used named below, to be
//! lifted onto `si-types` later; of the graphic properties only the texture cache is held.

use crate::effect::Fault;
use crate::tile::generate_tiled_pic;
use si_picture::{alpha_remove, border, crop, enlarge, resize, swap_bits_rgb, swap_bits_rgba, Irect, PictureRgba};
use si_x87::{trig, Round, F80};

/// Bytes of a texture map that the cache keeps a copy of (0x76 words).
pub const MAP_SIZE: usize = 0x1d8;
/// Offsets in `__SI_LclMap`.
pub const MAP_SCRIPT: usize = 0x10; // pointer: the picture a script texture shows
pub const MAP_NAME: usize = 0x14; // pointer: the picture file's name
pub const MAP_KIND: usize = 0x28; // 16 bits: 0, 1 a picture file; 2 a script texture
pub const MAP_METHOD: usize = 0x2a; // 16 bits: the mapping method; 5 and 6 are the ones that blend
pub const MAP_CROP: usize = 0x2e; // 16 bits: the crop rectangle is in force
pub const MAP_X0: usize = 0x30; // 16 bits each: first and last column, first and last row
pub const MAP_X1: usize = 0x32;
pub const MAP_Y0: usize = 0x34;
pub const MAP_Y1: usize = 0x36;
pub const MAP_BLEND: usize = 0x58; // float
pub const MAP_REPEAT_U: usize = 0xf8; // 16 bits each: repeats across and up
pub const MAP_REPEAT_V: usize = 0xfa;
pub const MAP_AXIS: usize = 0xfc; // 16 bits: which way the blend doubles the picture
pub const MAP_ALTERNATE_U: usize = 0xfe; // 16 bits each: every second repeat mirrored
pub const MAP_ALTERNATE_V: usize = 0x100;

/// A texture map: its first 0x1d8 bytes as they are in memory, and the two names its
/// pointers at 0x10 and 0x14 lead to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LclMap {
    pub raw: [u8; MAP_SIZE],
    pub script: Option<Vec<u8>>,
    pub name: Option<Vec<u8>>,
}

impl LclMap {
    pub fn i16_at(&self, at: usize) -> i16 {
        i16::from_le_bytes([self.raw[at], self.raw[at + 1]])
    }

    fn u32_at(raw: &[u8; MAP_SIZE], at: usize) -> u32 {
        u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]])
    }

    pub fn blend(&self) -> f32 {
        f32::from_bits(Self::u32_at(&self.raw, MAP_BLEND))
    }

    /// The name the cache files the map's picture under: the script's picture for a script
    /// texture, the file name otherwise.
    fn key(&self) -> Option<&[u8]> {
        if self.i16_at(MAP_KIND) == 2 { self.script.as_deref() } else { self.name.as_deref() }
    }
}

/// One entry of a window's texture cache: the original's 500-byte `SI_OptimisedTexture`
/// (name pointer at 0, a packed picture record at 4, a copy of the map at 0x14, four words
/// at 0x1ec).
#[derive(Clone, Debug, PartialEq)]
pub struct OptimisedTexture {
    pub name: Vec<u8>,
    pub picture: PictureRgba,
    pub map: [u8; MAP_SIZE],
    /// What the picture was made with: border, byte swap, tiling, alpha removed.
    pub made: [i16; 4],
}

/// The texture cache of a window's graphic properties (`SI_GraphicProps`: the number of
/// entries at 0x84, the entries at 0x88). Entries are used from the front; the first one
/// without a name ends the list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureCache {
    pub slots: Vec<Option<OptimisedTexture>>,
}

/// Files a map and the four flags in a cache entry [soft 0x085d5d74 optimTextureSet].
pub fn optim_texture_set(entry: &mut OptimisedTexture, map: &LclMap, border: i16, swap: i16, tile: i16, no_alpha: i16) {
    entry.map = map.raw;
    entry.made = [border, swap, tile, no_alpha];
}

/// Looks a map up in the cache [soft 0x085d5dc8 optimTextureFind]: `(true, i)` when entry
/// `i` was made from the same picture in the same way, else `(false, i)` with the first
/// free entry, or `(false, −1)` when the cache is full or there is none.
///
/// "The same" is: the same name; the same four flags; the same kind and method (the 32-bit
/// word at 0x28); the same crop switch, and with the switch on the same rectangle; the same
/// blend (as floats: not-a-number never matches); the same repeats, axis and first
/// alternate switch (the words at 0xf8 and 0xfc) and second alternate switch.
pub fn optim_texture_find(map: &LclMap, cache: Option<&TextureCache>, border: i16, swap: i16, tile: i16, no_alpha: i16) -> Result<(bool, i32), Fault> {
    let Some(cache) = cache else { return Ok((false, -1)) };
    let mut i = 0usize;
    let w = |raw: &[u8; MAP_SIZE], at: usize| LclMap::u32_at(raw, at);
    let s = |raw: &[u8; MAP_SIZE], at: usize| i16::from_le_bytes([raw[at], raw[at + 1]]);
    while let Some(Some(e)) = cache.slots.get(i) {
        // the comparison is `strcmp` with the map's name, whatever that pointer is
        let key = map.key().ok_or(Fault::NullPlane)?;
        let cstr = |b: &[u8]| b.iter().position(|&c| c == 0).map_or(b.len(), |n| n);
        let same = e.name[..cstr(&e.name)] == key[..cstr(key)]
            && e.made == [border, swap, tile, no_alpha]
            && w(&e.map, MAP_KIND) == w(&map.raw, MAP_KIND)
            && s(&e.map, MAP_CROP) == s(&map.raw, MAP_CROP)
            && (s(&e.map, MAP_CROP) == 0 || (w(&e.map, MAP_X0) == w(&map.raw, MAP_X0) && w(&e.map, MAP_Y0) == w(&map.raw, MAP_Y0)))
            && F80::from_f32(map.blend()).eq(F80::from_f32(f32::from_bits(w(&e.map, MAP_BLEND))))
            && w(&e.map, MAP_REPEAT_U) == w(&map.raw, MAP_REPEAT_U)
            && w(&e.map, MAP_AXIS) == w(&map.raw, MAP_AXIS)
            && s(&e.map, MAP_ALTERNATE_V) == s(&map.raw, MAP_ALTERNATE_V);
        if same {
            return Ok((true, i as i32));
        }
        i += 1;
    }
    Ok((false, if i < cache.slots.len() { i as i32 } else { -1 }))
}

/// The stock picture a script texture shows when it has none of its own: `noIcon` in the
/// resource directory.
pub fn no_icon(resdir: &[u8]) -> Vec<u8> {
    [resdir, b"/noIcon"].concat()
}

/// The picture of a script texture [soft 0x085d6784 loadScriptImage]: the map's own, or,
/// if it has none or that does not read, `noIcon` from the resource directory, **whose name
/// is then written into the map**. `read` stands for `SI_pictureReadRGBA` on a name.
pub fn load_script_image(map: &mut LclMap, resdir: &[u8], read: &mut dyn FnMut(&[u8]) -> Option<PictureRgba>) -> Option<PictureRgba> {
    if let Some(name) = map.script.clone() {
        if let Some(p) = read(&name) {
            return Some(p);
        }
        map.script = None;
    }
    let stock = no_icon(resdir);
    let p = read(&stock)?;
    map.script = Some(stock);
    Some(p)
}

/// 2^floor(log(v) / log(2)), as the loader computes it: both logarithms and the power
/// through the C library ([`si_x87::trig`], so that a check can replay the library's
/// answers), the first logarithm stored as a double, the quotient rounded down in the
/// register, the power truncated.
fn power_of_two_below(v: i32) -> i32 {
    let log_v = trig::log(v as f64).to_f64();
    let q = (F80::from_f64(log_v) / trig::log(2.0)).round_int(Round::Down).to_f64();
    trig::pow(2.0, q).to_i32(Round::Zero)
}

/// The largest texture side, from the environment variable `SI_SHADE_TEXSIZE_LIMIT`: 256
/// without it (or with text that is no number), no limit (0) for `OFF` or a number below 1,
/// else the number rounded down to a power of two. The original works this out once and
/// keeps it (`maxDimension`, 0x086fb7c0, −1 until then).
pub fn texture_size_limit(env: Option<&[u8]>) -> i32 {
    let Some(text) = env else { return 256 };
    if text == b"OFF" {
        return 0;
    }
    // sscanf "%d"
    let t = text.iter().position(|b| !b.is_ascii_whitespace()).map_or(&text[text.len()..], |i| &text[i..]);
    let (neg, digits) = match t.first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let n = digits.iter().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 {
        return 256;
    }
    let mut v: i64 = 0;
    for &b in &digits[..n] {
        v = (v * 10 + (b - b'0') as i64).min(i32::MAX as i64 + 1);
    }
    let v = if neg { (-v).max(i32::MIN as i64) } else { v.min(i32::MAX as i64) } as i32;
    if v > 0 {
        power_of_two_below(v)
    } else {
        0
    }
}

/// What the loader was asked for, beyond the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Black out the picture's outermost pixels.
    pub border: i16,
    /// Reverse the bytes of every pixel.
    pub swap: i16,
    /// Repeat the picture as the map says.
    pub tile: i16,
    /// Three bytes a pixel.
    pub no_alpha: i16,
}

/// A map's picture, made ready for the display [soft 0x085d6820 SI_picReadCrop].
///
/// 1. If the cache has it ([`optim_texture_find`]), a copy of the cached picture is returned.
/// 2. The picture is read: the file for kinds 0 and 1, [`load_script_image`] for kind 2;
///    **for any other kind nothing is read** and the steps below run on an empty record.
/// 3. With the crop switch on, and the rectangle not the whole picture: cut (`SI_cropPic`);
///    rows are counted from the top in the map, so the rectangle's rows run from height −
///    (the map's last row) to height − (its first).
/// 4. With `tile` and more than one repeat either way: [`generate_tiled_pic`], mode 1 for
///    alternate columns, 2 for rows, 3 for both.
/// 5. With a blend above 0.5, a method of 5 or 6, and no `SI_OLD_UV_CONVERSION` in the
///    environment: doubled (`SI_picEnlarge`).
/// 6. Resized, nearest pixel (`SI_resizePic`), to powers of two not above the width and the
///    height, each then halved until it is not above the limit ([`texture_size_limit`]).
/// 7. Border, alpha removal and byte swap as asked.
/// 8. Filed in the cache's first free entry, if there is one, with a copy of the map.
///
/// Steps 3, 5 and 6 replace the picture's size and pixels only: ratio, `fields` and the
/// spare word stay those of the picture read (step 4 clears them). `read` stands for
/// `SI_pictureReadRGBA` on a name; `limit` and `old_uv` for the two environment variables.
/// Nothing is returned when the picture does not read.
#[allow(clippy::too_many_arguments)]
pub fn pic_read_crop(map: &mut LclMap, o: Options, cache: Option<&mut TextureCache>, resdir: &[u8], limit: i32, old_uv: bool, read: &mut dyn FnMut(&[u8]) -> Option<PictureRgba>) -> Result<Option<PictureRgba>, Fault> {
    let (found, index) = optim_texture_find(map, cache.as_deref(), o.border, o.swap, o.tile, o.no_alpha)?;
    if found {
        let e = cache.as_ref().unwrap().slots[index as usize].as_ref().unwrap();
        let n = (e.picture.width as i32).wrapping_mul(e.picture.height as i32).wrapping_mul(if o.no_alpha != 0 { 3 } else { 4 });
        let mut p = e.picture.clone();
        p.data = e.picture.data.get(..n.max(0) as usize).ok_or(Fault::ShortPlane)?.to_vec();
        return Ok(Some(p));
    }
    let kind = map.i16_at(MAP_KIND);
    let mut pic = match kind {
        0 | 1 => match read(map.name.as_deref().ok_or(Fault::NullPlane)?) {
            Some(p) => p,
            None => return Ok(None),
        },
        2 => match load_script_image(map, resdir, read) {
            Some(p) => p,
            None => return Ok(None),
        },
        _ => PictureRgba::default(),
    };
    let replace = |pic: &mut PictureRgba, new: PictureRgba| {
        pic.width = new.width;
        pic.height = new.height;
        pic.data = new.data;
    };
    if map.i16_at(MAP_CROP) != 0 {
        let (x0, x1) = (map.i16_at(MAP_X0), map.i16_at(MAP_X1));
        // the map counts rows from the top: its first row is the rectangle's last
        let (y0, y1) = (pic.height.wrapping_sub(map.i16_at(MAP_Y1)), pic.height.wrapping_sub(map.i16_at(MAP_Y0)));
        if x1 as i32 - x0 as i32 + 1 != pic.width as i32 || y1 as i32 - y0 as i32 + 1 != pic.height as i32 {
            if let Some(cut) = crop(&pic, Irect { x0, x1, y0, y1 }) {
                replace(&mut pic, cut);
            }
        }
    }
    if o.tile != 0 && (map.i16_at(MAP_REPEAT_U) > 1 || map.i16_at(MAP_REPEAT_V) > 1) {
        let mode = (map.i16_at(MAP_ALTERNATE_U) != 0) as i16 + 2 * (map.i16_at(MAP_ALTERNATE_V) != 0) as i16;
        // an empty result leaves the original with a null picture, which the next step reads
        pic = generate_tiled_pic(&pic, map.i16_at(MAP_REPEAT_U) as i32, map.i16_at(MAP_REPEAT_V) as i32, mode).ok_or(Fault::NullPlane)?;
    }
    if F80::from_f32(0.5).lt(F80::from_f32(map.blend())) && (map.i16_at(MAP_METHOD) as u16).wrapping_sub(5) < 2 && !old_uv && pic.width > 0 && pic.height > 0 {
        // `SI_picEnlarge` gives nothing for an empty picture only; where the port of it
        // refuses (an axis word other than 0 and 1, a blend above 1), the original copies
        // outside its buffer
        let wide = enlarge(&pic, map.blend(), map.i16_at(MAP_AXIS)).ok_or(Fault::BadSize)?;
        replace(&mut pic, wide);
    }
    let mut w = power_of_two_below(pic.width as i32);
    let mut h = power_of_two_below(pic.height as i32);
    if limit > 0 {
        while limit < w {
            w /= 2;
        }
        while limit < h {
            h /= 2;
        }
    }
    if let Some(sized) = resize(&pic, w, h) {
        replace(&mut pic, sized);
    }
    if o.border != 0 {
        border(&mut pic);
    }
    if o.no_alpha != 0 {
        alpha_remove(&mut pic);
    }
    if o.swap != 0 {
        if o.no_alpha == 0 {
            swap_bits_rgba(&mut pic);
        } else {
            swap_bits_rgb(&mut pic);
        }
    }
    if let (Some(cache), true) = (cache, index != -1) {
        let n = (pic.width as i32).wrapping_mul(pic.height as i32).wrapping_mul(if o.no_alpha != 0 { 3 } else { 4 });
        let mut kept = pic.clone();
        kept.data = pic.data.get(..n.max(0) as usize).ok_or(Fault::ShortPlane)?.to_vec();
        let mut entry = OptimisedTexture { name: map.key().ok_or(Fault::NullPlane)?.to_vec(), picture: kept, map: [0; MAP_SIZE], made: [0; 4] };
        optim_texture_set(&mut entry, map, o.border, o.swap, o.tile, o.no_alpha);
        cache.slots[index as usize] = Some(entry);
    }
    Ok(Some(pic))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(name: &[u8]) -> LclMap {
        LclMap { raw: [0; MAP_SIZE], script: None, name: Some(name.to_vec()) }
    }

    fn picture(w: i16, h: i16) -> PictureRgba {
        PictureRgba { width: w, height: h, ratio: 1.0, fields: 3, pad: 0, data: (0..4 * w as usize * h as usize).map(|i| i as u8).collect() }
    }

    #[test]
    fn the_limit_from_the_environment() {
        assert_eq!(texture_size_limit(None), 256);
        assert_eq!(texture_size_limit(Some(b"OFF")), 0);
        assert_eq!(texture_size_limit(Some(b"300")), 256);
        assert_eq!(texture_size_limit(Some(b"100")), 64);
        // an exact power of two is the library's business: log(64) / log(2) may come out a
        // hair below 6, and then the answer is 32 (finding 0014, "Not determined")
        assert!([32, 64].contains(&texture_size_limit(Some(b"64"))));
        assert_eq!(texture_size_limit(Some(b"-4")), 0);
        assert_eq!(texture_size_limit(Some(b"big")), 256);
    }

    #[test]
    fn a_picture_comes_out_at_powers_of_two_and_is_cached() {
        let mut m = map(b"/p/a");
        let mut cache = TextureCache { slots: vec![None, None] };
        let o = Options { border: 0, swap: 0, tile: 0, no_alpha: 0 };
        let reads = std::cell::Cell::new(0);
        let mut read = |_: &[u8]| {
            reads.set(reads.get() + 1);
            Some(picture(6, 5))
        };
        let p = pic_read_crop(&mut m, o, Some(&mut cache), b"/res", 256, false, &mut read).unwrap().unwrap();
        assert_eq!((p.width, p.height, p.ratio, p.fields), (4, 4, 1.0, 3));
        assert_eq!(cache.slots[0].as_ref().unwrap().name, b"/p/a");
        let again = pic_read_crop(&mut m, o, Some(&mut cache), b"/res", 256, false, &mut read).unwrap().unwrap();
        assert_eq!(again, p);
        assert_eq!(reads.get(), 1);
        // made another way: not the same entry
        let bordered = pic_read_crop(&mut m, Options { border: 1, ..o }, Some(&mut cache), b"/res", 2, false, &mut read).unwrap().unwrap();
        assert_eq!((bordered.width, bordered.height, reads.get()), (2, 2, 2));
        assert!(cache.slots[1].is_some());
    }

    #[test]
    fn a_script_texture_falls_back_on_the_stock_picture_and_keeps_its_name() {
        let mut m = map(b"unused");
        m.raw[MAP_KIND] = 2;
        m.script = Some(b"/gone".to_vec());
        let mut read = |name: &[u8]| (name == b"/res/noIcon").then(|| picture(2, 2));
        assert!(load_script_image(&mut m, b"/res", &mut read).is_some());
        assert_eq!(m.script.as_deref(), Some(&b"/res/noIcon"[..]));
    }
}
