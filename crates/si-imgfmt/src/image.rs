// SPDX-License-Identifier: MIT
//! The picture library's C++ image classes (`ilImage*.c++`; finding 0014, "The image
//! classes", sixth pass): an image in memory with four bytes a pixel (`SI_ImageABGR`) or one
//! colour index a pixel (`SI_ImageCMAP`, with its median-cut colour reduction), the image
//! file and its formats (`SI_ImageFile`, `SI_ImageFileFormat`), the processors' registry
//! (`SI_ImageProcType`), and the start-up that loads the format and processor plug-ins
//! (`SI_ImageInit`).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `ilImageABGR.c++` (0x0861a0b0), `ilImageCMAP.c++`
//! (0x0861a5c0), `ilImageFile.c++` (0x0861bf70), `ilImageMem.c++` (0x0861c760),
//! `ilImageUtil.c++` (0x0861c990), `ilImage.c++` (0x0861ce20), `ilImageProc.c++` (0x0861ce80),
//! `ilImageSwap.c++` (0x0861cff0), and the classes' inline methods, which the compiler put at
//! the end of the program (0x086431a0–0x08644150).
//!
//! **The classes as Rust.** A C++ object is a record whose method table sits at offset 0x14
//! (`SI_Image` and everything derived from it). The virtual methods are trait methods here:
//! [`ImageMem`] for images in memory (its provided methods are `SI_ImageMem`'s own, which a
//! subclass overrides), [`ImageFile`] for image files (provided: `SI_ImageFile`'s), with the
//! method-table slot of each written beside it. The formats and processors themselves are not
//! in `soft`: they are the plug-ins `dso/sil/ilImage<NAME>.so` (the same-code scan finds none
//! of their functions in `soft`), which the program loads at run time and which subclass
//! `SI_ImageFile`, `SI_ImageFileFormat`, `SI_ImageProc` and `SI_ImageProcType`. A port of a
//! plug-in implements these traits; none is part of this crate.
//!
//! **Undefined corners.** Pixel accessors do not check their coordinates: outside the image
//! the original reads or writes past its buffer, and with no buffer (a size below 1) through a
//! null pointer. Here they return a [`Fault`]. Pixel buffers come from `malloc` and hold
//! whatever the allocator left until written; here they start at zero (nothing in the
//! library reads a pixel it has not written first). The class files are leaf code: the format
//! and processor lists (static class members) are values passed in ([`FormatRegistry`],
//! [`ProcRegistry`]), not globals of the memory image, since nothing else in the program
//! reaches them.

use crate::tiff::Fault;
use si_picture::PictureRgba;
use si_x87::{Round, F80};

/// The photometric number of a colour-mapped image (`SI_ImageCMAP` sets it; `imgconv` and
/// the save routines ask a file for it). The others in use: 1 (`SI_Image`'s default, RGB),
/// 2 (RGB with alpha, asked by the save routine), 4 (`SI_ImageABGR`). **[code]**
pub const PHOTOMETRIC_CMAP: i32 = 5;

fn fault_outside() -> Fault {
    Fault("a pixel outside the image: the original reads or writes past its buffer")
}

fn fault_null() -> Fault {
    Fault("an image without pixels (a size below 1): the original goes through a null pointer")
}

// ---------------------------------------------------------------------------------------------
// SI_Image

/// The fields of `SI_Image` (offsets 0 to 0x10; the method table is at 0x14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub samples_per_pixel: i32,
    pub bits_per_sample: i32,
    pub photometric: i32,
}

impl Default for Image {
    fn default() -> Self {
        Image::new()
    }
}

impl Image {
    /// [soft 0x0861ce20 SI_Image::SI_Image]: size 0 × 0, three samples of 8 bits, photometric
    /// 1. [soft 0x0861ce54 SI_Image::~SI_Image] frees nothing of its own (it deletes the object
    /// when asked to): the value going out of scope.
    pub fn new() -> Image {
        Image { width: 0, height: 0, samples_per_pixel: 3, bits_per_sample: 8, photometric: 1 }
    }
}

/// `SI_Image`'s methods, virtual from slot 0xc: every subclass (and plug-in) may override the
/// setters; the provided ones are `SI_Image`'s inline methods, which store and answer 1.
pub trait ImageBase {
    fn image(&self) -> &Image;
    fn image_mut(&mut self) -> &mut Image;

    /// Slot 0xc [soft 0x08643fe4 SI_Image::setWidth].
    fn set_width(&mut self, v: i32) -> i32 {
        self.image_mut().width = v;
        1
    }
    /// Slot 0x10 [soft 0x08643ff8 SI_Image::setHeight].
    fn set_height(&mut self, v: i32) -> i32 {
        self.image_mut().height = v;
        1
    }
    /// Slot 0x14 [soft 0x0864402c SI_Image::setSamplesPerPixel].
    fn set_samples_per_pixel(&mut self, v: i32) -> i32 {
        self.image_mut().samples_per_pixel = v;
        1
    }
    /// Slot 0x18 [soft 0x08644044 SI_Image::setBitsPerSample].
    fn set_bits_per_sample(&mut self, v: i32) -> i32 {
        self.image_mut().bits_per_sample = v;
        1
    }
    /// Slot 0x1c [soft 0x0864405c SI_Image::setBytesPerSample]: stores eight times the value
    /// as the bits.
    fn set_bytes_per_sample(&mut self, v: i32) -> i32 {
        self.image_mut().bits_per_sample = v.wrapping_shl(3);
        1
    }
    /// Slot 0x20 [soft 0x08644074 SI_Image::setPhotoMetric]. A plug-in's version answers 0
    /// for a photometric its format cannot hold; `imgconv` and the save routines ask that way.
    fn set_photometric(&mut self, v: i32) -> i32 {
        self.image_mut().photometric = v;
        1
    }
    /// [soft 0x08644010 SI_Image::getWidth]
    fn width(&self) -> i32 {
        self.image().width
    }
    /// [soft 0x0864401c SI_Image::getHeight]
    fn height(&self) -> i32 {
        self.image().height
    }
    /// [soft 0x0864408c SI_Image::getSamplesPerPixel]
    fn samples_per_pixel(&self) -> i32 {
        self.image().samples_per_pixel
    }
    /// [soft 0x0864409c SI_Image::getBitsPerSample]
    fn bits_per_sample(&self) -> i32 {
        self.image().bits_per_sample
    }
    /// [soft 0x086440ac SI_Image::getBytesPerSample]: the bits divided by 8, rounded toward
    /// zero (a negative count of bits gives a negative count of bytes).
    fn bytes_per_sample(&self) -> i32 {
        self.image().bits_per_sample / 8
    }
    /// [soft 0x086440c4 SI_Image::getPhotoMetric]
    fn photometric(&self) -> i32 {
        self.image().photometric
    }
}

/// [soft 0x0861cff0 SI_ImageSwapFloat]: the float with its four bytes in the other order (a
/// big-endian float of a file read on this machine).
pub fn swap_float(v: f32) -> f32 {
    f32::from_bits(v.to_bits().swap_bytes())
}

// ---------------------------------------------------------------------------------------------
// SI_ImageMem

/// The pixel buffer of an image in memory (`SI_ImageMem::getBuffer`'s answer).
pub enum Buffer<'a> {
    /// No buffer: the null pointer of an image whose size is below 1.
    None,
    /// `SI_ImageABGR`: four bytes a pixel.
    Bytes(&'a [u8]),
    /// `SI_ImageCMAP`: one 16-bit index a pixel.
    Words(&'a [u16]),
}

/// What the typed pixel accessors (slots 0x4c and 0x50) carry: their four pointers are
/// `unsigned char *` in the declaration and read as bytes, 16-bit words or floats by kind
/// (1, 2 or 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Channels {
    U8([Option<u8>; 4]),
    U16([Option<u16>; 4]),
    F32([Option<f32>; 4]),
}

/// An image in memory: `SI_ImageMem`'s virtual methods (slots 0x24 to 0x80). Provided
/// methods are `SI_ImageMem`'s own; the pixel accessors from 0x34 to 0x50 are pure virtual
/// there. [soft 0x0861c760 SI_ImageMem::SI_ImageMem] and [soft 0x0861c784
/// SI_ImageMem::~SI_ImageMem] only set the method table over `SI_Image`'s: an implementor's
/// construction and drop.
pub trait ImageMem: ImageBase {
    /// Slot 0x24 [soft 0x08643eb0 SI_ImageMem::getBuffer].
    fn buffer(&self) -> Buffer<'_>;
    /// Slot 0x28: pure virtual in `SI_ImageMem`.
    fn set_size(&mut self, width: i32, height: i32);
    /// Slot 0x34: red, green, blue, alpha as bytes. (The original takes four pointers and
    /// skips a null one; here the four values come back.)
    fn get_pixel(&self, x: i32, y: i32) -> Result<[u8; 4], Fault>;
    /// Slot 0x38.
    fn set_pixel(&mut self, x: i32, y: i32, v: [u8; 4]) -> Result<(), Fault>;
    /// Slot 0x3c: 16-bit samples.
    fn get_pixel16(&self, x: i32, y: i32) -> Result<[u16; 4], Fault>;
    /// Slot 0x40.
    fn set_pixel16(&mut self, x: i32, y: i32, v: [u16; 4]) -> Result<(), Fault>;
    /// Slot 0x44: samples as floats from 0 to 1.
    fn get_pixel_f(&self, x: i32, y: i32) -> Result<[f32; 4], Fault>;
    /// Slot 0x48.
    fn set_pixel_f(&mut self, x: i32, y: i32, v: [f32; 4]) -> Result<(), Fault>;
    /// Slot 0x4c: kind 1, 2 or 4 picks slot 0x34, 0x3c or 0x44; another kind does nothing
    /// (`None`).
    fn get_pixel_as(&self, x: i32, y: i32, kind: i32) -> Result<Option<Channels>, Fault>;
    /// Slot 0x50: the typed setter; an absent channel takes the class's default.
    fn set_pixel_as(&mut self, x: i32, y: i32, v: &Channels, kind: i32) -> Result<(), Fault>;

    /// Slot 0x2c [soft 0x0861c7a8 SI_ImageMem::readTile]: copies the tile (`x`, `y`, `w`,
    /// `h`) of this image to `dst` at (`dx`, `dy`) pixel by pixel through the float
    /// accessors. The clipping is the original's slip: it compares `x + w` with `w` (not with
    /// the width), so a tile is shortened by `x` when `x` is above 0 and by `y` when `y` is,
    /// and never clipped to the image. Answers whether neither was shortened.
    fn read_tile(&mut self, x: i32, y: i32, w: i32, h: i32, dst: &mut dyn ImageMem, dx: i32, dy: i32) -> Result<bool, Fault> {
        let (w, h, whole) = mem_tile_clip(x, y, w, h);
        for j in 0..h {
            for i in 0..w {
                let v = self.get_pixel_f(x + i, y + j)?;
                dst.set_pixel_f(dx + i, dy + j, v)?;
            }
        }
        Ok(whole)
    }
    /// Slot 0x30 [soft 0x0861c89c SI_ImageMem::writeTile]: the reverse, from `src` at
    /// (`sx`, `sy`) into this image, with the same clipping.
    fn write_tile(&mut self, x: i32, y: i32, w: i32, h: i32, src: &mut dyn ImageMem, sx: i32, sy: i32) -> Result<bool, Fault> {
        let (w, h, whole) = mem_tile_clip(x, y, w, h);
        for j in 0..h {
            for i in 0..w {
                let v = src.get_pixel_f(sx + i, sy + j)?;
                self.set_pixel_f(x + i, y + j, v)?;
            }
        }
        Ok(whole)
    }
    /// Slot 0x54 [soft 0x08643ec0 SI_ImageMem::getPixel(float *)]: the grey value 0.11 blue
    /// + 0.59 green + 0.30 red (float constants, summed in the x87 unit in that order, stored
    /// as a float).
    fn get_grey(&self, x: i32, y: i32) -> Result<f32, Fault> {
        let [r, g, b, _] = self.get_pixel_f(x, y)?;
        let s = F80::from(b) * 0.11f32 + F80::from(g) * 0.59f32;
        Ok((s + F80::from(r) * 0.3f32).to_f32())
    }
    /// Slot 0x58 [soft 0x08643f20 SI_ImageMem::setPixel(float)]: the value in red, green and
    /// blue, alpha 1.
    fn set_grey(&mut self, x: i32, y: i32, v: f32) -> Result<(), Fault> {
        self.set_pixel_f(x, y, [v, v, v, 1.0])
    }
    /// Slot 0x5c [soft 0x08643f60 SI_ImageMem::getCPixel]: 0.
    fn get_cpixel(&self, _x: i32, _y: i32) -> Result<i32, Fault> {
        Ok(0)
    }
    /// Slot 0x60 [soft 0x08643f6c SI_ImageMem::setCPixel]: nothing.
    fn set_cpixel(&mut self, _x: i32, _y: i32, _v: i32) -> Result<(), Fault> {
        Ok(())
    }
    /// Slot 0x64 [soft 0x08643f74 SI_ImageMem::getColorIndexValue(int, uchar *, uchar *,
    /// uchar *)]: nothing is written (`None`: the caller's variables keep what they held).
    fn get_color_index_value3(&self, _i: i32) -> Result<Option<[u8; 3]>, Fault> {
        Ok(None)
    }
    /// Slot 0x68 [soft 0x08643f7c SI_ImageMem::setColorIndexValue(int, uchar, uchar, uchar)]:
    /// nothing.
    fn set_color_index_value3(&mut self, _i: i32, _v: [u8; 3]) -> Result<(), Fault> {
        Ok(())
    }
    /// Slot 0x6c [soft 0x08643f84 SI_ImageMem::getColorIndexValue(int, uchar *, uchar *,
    /// uchar *, uchar *)]: nothing is written.
    fn get_color_index_value4(&self, _i: i32) -> Result<Option<[u8; 4]>, Fault> {
        Ok(None)
    }
    /// Slot 0x70 [soft 0x08643f8c SI_ImageMem::setColorIndexValue(int, uchar, uchar, uchar,
    /// uchar)]: nothing.
    fn set_color_index_value4(&mut self, _i: i32, _v: [u8; 4]) -> Result<(), Fault> {
        Ok(())
    }
    /// Slot 0x74 [soft 0x08643f94 SI_ImageMem::copyColorMap]: nothing.
    fn copy_color_map(&self, _first: i32, _last: i32, _to: i32, _dst: &mut dyn ImageMem) -> Result<(), Fault> {
        Ok(())
    }
    /// Slot 0x78 [soft 0x08643f9c SI_ImageMem::generateColorIndex]: nothing.
    fn generate_color_index(&mut self, _src: &dyn ImageMem) -> Result<(), Fault> {
        Ok(())
    }
    /// Slot 0x7c [soft 0x08643fa4 SI_ImageMem::getIndexMaxValue]: 0.
    fn index_max_value(&self) -> i32 {
        0
    }
    /// Slot 0x80 [soft 0x08643fb0 SI_ImageMem::setIndexMaxValue]: nothing.
    fn set_index_max_value(&mut self, _v: i32) {}
}

/// The clipping of `SI_ImageMem`'s tile copies (see [`ImageMem::read_tile`]).
fn mem_tile_clip(x: i32, y: i32, mut w: i32, mut h: i32) -> (i32, i32, bool) {
    let mut whole = true;
    if x.wrapping_add(w) > w {
        whole = false;
        w = w.wrapping_sub(x);
    }
    if y.wrapping_add(h) > h {
        whole = false;
        h = h.wrapping_sub(y);
    }
    (w, h, whole)
}

/// The clipping of the subclasses' tile copies: to the image's own width and height, from
/// `x` and `y` (which are not checked against 0).
fn tile_clip(img: &Image, x: i32, y: i32, mut w: i32, mut h: i32) -> (i32, i32, bool) {
    let mut whole = true;
    if x.wrapping_add(w) > img.width {
        whole = false;
        w = img.width.wrapping_sub(x);
    }
    if y.wrapping_add(h) > img.height {
        whole = false;
        h = img.height.wrapping_sub(y);
    }
    (w, h, whole)
}

/// `(int)` of an x87 value: the conversions below set the rounding to truncation.
fn trunc(v: F80) -> i32 {
    v.to_i32(Round::Zero)
}

/// A byte sample as a float: `(float)v / 255.0f`, in the x87 unit, stored as a float.
fn byte_to_float(v: u8) -> f32 {
    (F80::from(v as i32) / 255.0f32).to_f32()
}

/// A float sample as a byte: `(int)(v * 255.0) & 0xff`.
fn float_to_byte(v: f32) -> u8 {
    trunc(F80::from(v) * 255.0f64) as u8
}

// ---------------------------------------------------------------------------------------------
// SI_ImageABGR

/// `SI_ImageABGR` (0x1c bytes): four bytes a pixel in the order alpha, blue, green, red, row
/// after row, row 0 first, at 0x18.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageAbgr {
    pub image: Image,
    /// `None`: the null pointer of a size below 1.
    pub pixels: Option<Vec<u8>>,
}

impl ImageAbgr {
    /// [soft 0x0861a0b0 SI_ImageABGR::SI_ImageABGR]: four samples of 8 bits, photometric 4, a
    /// buffer of `4 × width × height` bytes (none for a size below 1). The size is stored as
    /// given. [soft 0x0861a150 SI_ImageABGR::~SI_ImageABGR] frees the buffer: the value going
    /// out of scope.
    pub fn new(width: i32, height: i32) -> ImageAbgr {
        let pixels = (width >= 1 && height >= 1).then(|| vec![0u8; buffer_len(width, height, 4)]);
        ImageAbgr { image: Image { width, height, samples_per_pixel: 4, bits_per_sample: 8, photometric: 4 }, pixels }
    }

    fn offset(&self, x: i32, y: i32) -> Result<usize, Fault> {
        let buf = self.pixels.as_ref().ok_or_else(fault_null)?;
        let i = (y as i64 * self.image.width as i64 + x as i64) * 4;
        if i < 0 || i as usize + 4 > buf.len() {
            return Err(fault_outside());
        }
        Ok(i as usize)
    }

    /// [soft 0x0861a200 SI_ImageABGR::readTile(…, SI_ImageABGR *, …)]: the overload for two
    /// such images copies rows with `memcpy`, and **from `other` into this image** (the
    /// opposite direction of the virtual `readTile`). The tile is clipped to this image; in
    /// `other` a row is taken to be `w` pixels long (the tile's width as asked, not `other`'s
    /// width). Answers whether nothing was clipped.
    pub fn read_tile_abgr(&mut self, x: i32, y: i32, w: i32, h: i32, other: &ImageAbgr, ox: i32, oy: i32) -> Result<bool, Fault> {
        let (cw, ch, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..ch {
            let n = cw.max(0) as usize * 4;
            let s = abgr_raw_offset(other, ox, oy + j, w, n)?;
            let d = abgr_raw_offset(self, x, y + j, self.image.width, n)?;
            let row: Vec<u8> = other.pixels.as_ref().unwrap()[s..s + n].to_vec();
            self.pixels.as_mut().unwrap()[d..d + n].copy_from_slice(&row);
        }
        Ok(whole)
    }

    /// [soft 0x0861a2a4 SI_ImageABGR::writeTile(…, SI_ImageABGR *, …)]: the same, from this
    /// image into `other`.
    pub fn write_tile_abgr(&self, x: i32, y: i32, w: i32, h: i32, other: &mut ImageAbgr, ox: i32, oy: i32) -> Result<bool, Fault> {
        let (cw, ch, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..ch {
            let n = cw.max(0) as usize * 4;
            let s = abgr_raw_offset(self, x, y + j, self.image.width, n)?;
            let d = abgr_raw_offset(other, ox, oy + j, w, n)?;
            let row: Vec<u8> = self.pixels.as_ref().unwrap()[s..s + n].to_vec();
            other.pixels.as_mut().unwrap()[d..d + n].copy_from_slice(&row);
        }
        Ok(whole)
    }
}

fn buffer_len(width: i32, height: i32, size: i64) -> usize {
    (width as i64 * height as i64 * size).max(0) as usize
}

/// The byte offset of pixel (`x`, `y`) of a buffer whose rows are `stride` pixels, checked for
/// `n` bytes.
fn abgr_raw_offset(img: &ImageAbgr, x: i32, y: i32, stride: i32, n: usize) -> Result<usize, Fault> {
    let buf = img.pixels.as_ref().ok_or_else(fault_null)?;
    let i = (y as i64 * stride as i64 + x as i64) * 4;
    if i < 0 || i as usize + n > buf.len() {
        return Err(fault_outside());
    }
    Ok(i as usize)
}

impl ImageBase for ImageAbgr {
    fn image(&self) -> &Image {
        &self.image
    }
    fn image_mut(&mut self) -> &mut Image {
        &mut self.image
    }
}

impl ImageMem for ImageAbgr {
    fn buffer(&self) -> Buffer<'_> {
        match &self.pixels {
            Some(p) => Buffer::Bytes(p),
            None => Buffer::None,
        }
    }

    /// [soft 0x0861a188 SI_ImageABGR::setSize]: a size below 1 frees the buffer; another
    /// reallocates it (what it held is kept as far as it goes; the rest is undefined in the
    /// original, zero here). The width and height go through the virtual setters.
    fn set_size(&mut self, width: i32, height: i32) {
        if width < 1 || height < 1 {
            self.pixels = None;
        } else {
            let n = buffer_len(width, height, 4);
            self.pixels.get_or_insert_with(Vec::new).resize(n, 0);
        }
        self.set_width(width);
        self.set_height(height);
    }

    /// [soft 0x0861a568 SI_ImageABGR::getPixel(uchar)]
    fn get_pixel(&self, x: i32, y: i32) -> Result<[u8; 4], Fault> {
        let i = self.offset(x, y)?;
        let p = &self.pixels.as_ref().unwrap()[i..i + 4];
        Ok([p[3], p[2], p[1], p[0]])
    }

    /// [soft 0x0861a528 SI_ImageABGR::setPixel(uchar)]
    fn set_pixel(&mut self, x: i32, y: i32, v: [u8; 4]) -> Result<(), Fault> {
        let i = self.offset(x, y)?;
        let p = &mut self.pixels.as_mut().unwrap()[i..i + 4];
        p[3] = v[0];
        p[2] = v[1];
        p[1] = v[2];
        p[0] = v[3];
        Ok(())
    }

    /// [soft 0x086431d8 SI_ImageABGR::getPixel(ushort)]: each byte twice (v × 257).
    fn get_pixel16(&self, x: i32, y: i32) -> Result<[u16; 4], Fault> {
        Ok(self.get_pixel(x, y)?.map(|v| v as u16 * 0x101))
    }

    /// [soft 0x08643270 SI_ImageABGR::setPixel(ushort)]: the high byte of each.
    fn set_pixel16(&mut self, x: i32, y: i32, v: [u16; 4]) -> Result<(), Fault> {
        self.set_pixel(x, y, v.map(|c| (c >> 8) as u8))
    }

    /// [soft 0x086432ac SI_ImageABGR::getPixel(float)]: byte / 255.
    fn get_pixel_f(&self, x: i32, y: i32) -> Result<[f32; 4], Fault> {
        Ok(self.get_pixel(x, y)?.map(byte_to_float))
    }

    /// [soft 0x08643350 SI_ImageABGR::setPixel(float)]: `(int)(v × 255.0)`, its low byte.
    fn set_pixel_f(&mut self, x: i32, y: i32, v: [f32; 4]) -> Result<(), Fault> {
        self.set_pixel(x, y, v.map(float_to_byte))
    }

    /// [soft 0x08643420 SI_ImageABGR::getPixel(uchar *, …, int)]
    fn get_pixel_as(&self, x: i32, y: i32, kind: i32) -> Result<Option<Channels>, Fault> {
        typed_get(self, x, y, kind)
    }

    /// [soft 0x086434c0 SI_ImageABGR::setPixel(uchar *, …, int)]: an absent colour is 0, an
    /// absent alpha opaque (0xff, 0xffff, 1.0).
    fn set_pixel_as(&mut self, x: i32, y: i32, v: &Channels, kind: i32) -> Result<(), Fault> {
        typed_set(self, x, y, v, kind, true)
    }

    /// [soft 0x0861a34c SI_ImageABGR::readTile(…, SI_ImageMem *, …)]: pixel by pixel from
    /// this image into `dst` through the byte accessors, the tile clipped to this image.
    fn read_tile(&mut self, x: i32, y: i32, w: i32, h: i32, dst: &mut dyn ImageMem, dx: i32, dy: i32) -> Result<bool, Fault> {
        let (w, h, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..h {
            for i in 0..w {
                let v = self.get_pixel(x + i, y + j)?;
                dst.set_pixel(dx + i, dy + j, v)?;
            }
        }
        Ok(whole)
    }

    /// [soft 0x0861a438 SI_ImageABGR::writeTile(…, SI_ImageMem *, …)]: the reverse.
    fn write_tile(&mut self, x: i32, y: i32, w: i32, h: i32, src: &mut dyn ImageMem, sx: i32, sy: i32) -> Result<bool, Fault> {
        let (w, h, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..h {
            for i in 0..w {
                let v = src.get_pixel(sx + i, sy + j)?;
                self.set_pixel(x + i, y + j, v)?;
            }
        }
        Ok(whole)
    }
}

/// The typed getter of both subclasses: kind 1, 2, 4 pick the byte, 16-bit and float
/// accessors.
fn typed_get<T: ImageMem + ?Sized>(img: &T, x: i32, y: i32, kind: i32) -> Result<Option<Channels>, Fault> {
    Ok(match kind {
        1 => Some(Channels::U8(img.get_pixel(x, y)?.map(Some))),
        2 => Some(Channels::U16(img.get_pixel16(x, y)?.map(Some))),
        4 => Some(Channels::F32(img.get_pixel_f(x, y)?.map(Some))),
        _ => None,
    })
}

/// The typed setter of both subclasses. The pointers are read as the kind says whatever the
/// caller passed; here a value of another kind than `kind` is taken as absent. `opaque`: an
/// absent alpha is full (`SI_ImageABGR`) or 0 (`SI_ImageCMAP`).
fn typed_set<T: ImageMem + ?Sized>(img: &mut T, x: i32, y: i32, v: &Channels, kind: i32, opaque: bool) -> Result<(), Fault> {
    match kind {
        1 => {
            let c = if let Channels::U8(c) = v { *c } else { [None; 4] };
            let a = if opaque { 0xff } else { 0 };
            img.set_pixel(x, y, [c[0].unwrap_or(0), c[1].unwrap_or(0), c[2].unwrap_or(0), c[3].unwrap_or(a)])
        }
        2 => {
            let c = if let Channels::U16(c) = v { *c } else { [None; 4] };
            let a = if opaque { 0xffff } else { 0 };
            img.set_pixel16(x, y, [c[0].unwrap_or(0), c[1].unwrap_or(0), c[2].unwrap_or(0), c[3].unwrap_or(a)])
        }
        4 => {
            let c = if let Channels::F32(c) = v { *c } else { [None; 4] };
            let a = if opaque { 1.0 } else { 0.0 };
            img.set_pixel_f(x, y, [c[0].unwrap_or(0.0), c[1].unwrap_or(0.0), c[2].unwrap_or(0.0), c[3].unwrap_or(a)])
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// SI_ImageCMAP

/// One box of the median cut (`SI_CMAP_Sub_Region`, six bytes): least and greatest red, green
/// and blue, in cells of the 64-level histogram.
pub type Region = [u8; 6];

/// The colour histogram of the median cut: 64 × 64 × 64 counts, red the slowest index.
pub struct Histogram(Vec<u32>);

impl Histogram {
    fn at(&self, r: u8, g: u8, b: u8) -> Result<u32, Fault> {
        let i = r as usize * 4096 + g as usize * 64 + b as usize;
        // a box beyond 63 reads past the megabyte; not reachable from generateColorIndex
        self.0.get(i).copied().ok_or(Fault("a histogram cell beyond 63: the original reads past its table"))
    }
}

/// [soft 0x0861a5c0 SI_Bounding_Box]: the smallest box inside (`r0`…`r1`, `g0`…`g1`,
/// `b0`…`b1`) holding every occupied cell. Starts from the box turned inside out (least =
/// the upper bound, greatest = the lower), so an empty box comes back inverted.
pub fn bounding_box(h: &Histogram, r0: u8, r1: u8, g0: u8, g1: u8, b0: u8, b1: u8) -> Result<Region, Fault> {
    let mut b = [r1, r0, g1, g0, b1, b0];
    let mut r = r0;
    while r <= r1 {
        let mut g = g0;
        while g <= g1 {
            let mut c = b0;
            while c <= b1 {
                if h.at(r, g, c)? != 0 {
                    b[0] = b[0].min(r);
                    b[2] = b[2].min(g);
                    b[4] = b[4].min(c);
                    b[1] = b[1].max(r);
                    b[3] = b[3].max(g);
                    b[5] = b[5].max(c);
                }
                if c == 255 {
                    return Err(Fault("a box bound of 255: the original's byte loop never ends"));
                }
                c += 1;
            }
            if g == 255 {
                return Err(Fault("a box bound of 255: the original's byte loop never ends"));
            }
            g += 1;
        }
        if r == 255 {
            return Err(Fault("a box bound of 255: the original's byte loop never ends"));
        }
        r += 1;
    }
    Ok(b)
}

/// Sums over the occupied cells of a box: (Σ red, Σ green, Σ blue, count), each cell weighted
/// by `weight(count)`. The sums are 32-bit, unsigned, wrapping.
fn box_sums(h: &Histogram, r0: u8, r1: u8, g0: u8, g1: u8, b0: u8, b1: u8, weighted: bool) -> Result<[u32; 4], Fault> {
    if r1 == 255 || g1 == 255 || b1 == 255 {
        return Err(Fault("a box bound of 255: the original's byte loop never ends"));
    }
    let mut s = [0u32; 4];
    let mut r = r0 as u32;
    while r <= r1 as u32 {
        let mut g = g0 as u32;
        while g <= g1 as u32 {
            let mut b = b0 as u32;
            while b <= b1 as u32 {
                let n = h.at(r as u8, g as u8, b as u8)?;
                if n != 0 {
                    let k = if weighted { n } else { 1 };
                    s[0] = s[0].wrapping_add(r.wrapping_mul(k));
                    s[1] = s[1].wrapping_add(g.wrapping_mul(k));
                    s[2] = s[2].wrapping_add(b.wrapping_mul(k));
                    s[3] = s[3].wrapping_add(k);
                }
                b += 1;
            }
            g += 1;
        }
        r += 1;
    }
    if s[3] == 0 {
        return Err(Fault("an empty box: the original divides by zero"));
    }
    Ok(s)
}

/// [soft 0x0861a6ac SI_Median_Box]: the mean place of the occupied cells of a box (each cell
/// counted once, whatever its count), less the box's lower bounds: three bytes. The original
/// divides by the number of cells, so an empty box stops it (divide error).
pub fn median_box(h: &Histogram, r0: u8, r1: u8, g0: u8, g1: u8, b0: u8, b1: u8) -> Result<[u8; 3], Fault> {
    let s = box_sums(h, r0, r1, g0, g1, b0, b1, false)?;
    Ok([
        ((s[0] / s[3]) as u8).wrapping_sub(r0),
        ((s[1] / s[3]) as u8).wrapping_sub(g0),
        ((s[2] / s[3]) as u8).wrapping_sub(b0),
    ])
}

/// [soft 0x0861a7b4 SI_Color_Ponderated_Mean]: the mean colour of a box, each cell weighted
/// by its count (32-bit unsigned sums, wrapping), in histogram cells.
pub fn color_ponderated_mean(h: &Histogram, b: &Region) -> Result<[u8; 3], Fault> {
    let s = box_sums(h, b[0], b[1], b[2], b[3], b[4], b[5], true)?;
    Ok([(s[0] / s[3]) as u8, (s[1] / s[3]) as u8, (s[2] / s[3]) as u8])
}

/// `SI_ImageCMAP` (0x34 bytes): one 16-bit colour index a pixel at 0x18, four colour tables
/// (red, green, blue, alpha, one byte an entry) at 0x1c to 0x28, the remapping quality at
/// 0x2c and the greatest index at 0x30.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageCmap {
    pub image: Image,
    /// `None`: the null pointer of a size below 1.
    pub indices: Option<Vec<u16>>,
    /// Red, green, blue, alpha: `colours` bytes each (`calloc`: zero), `None` for a count of
    /// 0 or less (the C library's answer to `calloc` of such a count is taken as no table).
    pub tables: [Option<Vec<u8>>; 4],
    /// 1 (the constructor's): `writeTile` maps pixels through a coarse grid of the colours;
    /// 0: through `setPixel`'s search of every colour; another value: `writeTile` does
    /// nothing.
    pub remap_quality: i32,
    /// The constructor's count of colours less 1. **The median cut makes this many colours**
    /// and the mapping searches this many: the last entry of the table is never used.
    pub max_index: i32,
}

impl ImageCmap {
    /// [soft 0x0861a8c0 SI_ImageCMAP::SI_ImageCMAP(int colours, int width, int height)]:
    /// tables of `colours` zero bytes, indices for `width × height` pixels (none for a size
    /// below 1), remapping quality 1, greatest index `colours − 1`, one sample of 8 bits,
    /// photometric 5. [soft 0x0861a9ac SI_ImageCMAP::~SI_ImageCMAP] frees the five blocks:
    /// the value going out of scope.
    pub fn new(colours: i32, width: i32, height: i32) -> ImageCmap {
        let table = || (colours > 0).then(|| vec![0u8; colours as usize]);
        ImageCmap {
            image: Image { width, height, samples_per_pixel: 1, bits_per_sample: 8, photometric: PHOTOMETRIC_CMAP },
            indices: (width >= 1 && height >= 1).then(|| vec![0u16; buffer_len(width, height, 1)]),
            tables: [table(), table(), table(), table()],
            remap_quality: 1,
            max_index: colours.wrapping_sub(1),
        }
    }

    /// [soft 0x08643668 SI_ImageCMAP::getRemapQuality]
    pub fn remap_quality(&self) -> i32 {
        self.remap_quality
    }

    /// [soft 0x08643678 SI_ImageCMAP::setRemapQuality]
    pub fn set_remap_quality(&mut self, v: i32) {
        self.remap_quality = v;
    }

    fn slot(&self, x: i32, y: i32) -> Result<usize, Fault> {
        let buf = self.indices.as_ref().ok_or_else(fault_null)?;
        let i = y as i64 * self.image.width as i64 + x as i64;
        if i < 0 || i as usize >= buf.len() {
            return Err(fault_outside());
        }
        Ok(i as usize)
    }

    fn entry(&self, c: usize, i: i64) -> Result<u8, Fault> {
        let t = self.tables[c].as_ref().ok_or(Fault("a colour table of no entries: the original reads through a null pointer"))?;
        if i < 0 || i as usize >= t.len() {
            return Err(Fault("a colour index beyond the table: the original reads past it"));
        }
        Ok(t[i as usize])
    }

    fn set_entry(&mut self, c: usize, i: i64, v: u8) -> Result<(), Fault> {
        let t = self.tables[c].as_mut().ok_or(Fault("a colour table of no entries: the original writes through a null pointer"))?;
        if i < 0 || i as usize >= t.len() {
            return Err(Fault("a colour index beyond the table: the original writes past it"));
        }
        t[i as usize] = v;
        Ok(())
    }

    /// [soft 0x0861b2bc SI_ImageCMAP::readTile(…, SI_ImageCMAP *, …)]: rows of indices copied
    /// with `memcpy` **from `other` into this image**, clipped to this image, rows of `other`
    /// taken as `w` indices long (as [`ImageAbgr::read_tile_abgr`]).
    pub fn read_tile_cmap(&mut self, x: i32, y: i32, w: i32, h: i32, other: &ImageCmap, ox: i32, oy: i32) -> Result<bool, Fault> {
        let (cw, ch, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..ch {
            let n = cw.max(0) as usize;
            let s = cmap_raw_offset(other, ox, oy + j, w, n)?;
            let d = cmap_raw_offset(self, x, y + j, self.image.width, n)?;
            let row: Vec<u16> = other.indices.as_ref().unwrap()[s..s + n].to_vec();
            self.indices.as_mut().unwrap()[d..d + n].copy_from_slice(&row);
        }
        Ok(whole)
    }

    /// [soft 0x0861b35c SI_ImageCMAP::writeTile(…, SI_ImageCMAP *, …)]: the same, from this
    /// image into `other`.
    pub fn write_tile_cmap(&self, x: i32, y: i32, w: i32, h: i32, other: &mut ImageCmap, ox: i32, oy: i32) -> Result<bool, Fault> {
        let (cw, ch, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..ch {
            let n = cw.max(0) as usize;
            let s = cmap_raw_offset(self, x, y + j, self.image.width, n)?;
            let d = cmap_raw_offset(other, ox, oy + j, w, n)?;
            let row: Vec<u16> = self.indices.as_ref().unwrap()[s..s + n].to_vec();
            other.indices.as_mut().unwrap()[d..d + n].copy_from_slice(&row);
        }
        Ok(whole)
    }

    /// The median cut of [`ImageMem::generate_color_index`], given the histogram.
    fn median_cut(&mut self, hist: &Histogram) -> Result<(), Fault> {
        let n = self.max_index as u32;
        let mut occupied = 0u32;
        for i in 0..64 * 64 * 64 {
            if hist.0[i] != 0 {
                occupied += 1;
            }
        }
        if occupied < n {
            // fewer occupied cells than colours: one colour a cell, in cell order
            let mut k = 0i64;
            for r in 0..64u8 {
                for g in 0..64u8 {
                    for b in 0..64u8 {
                        if hist.at(r, g, b)? != 0 {
                            self.set_entry(0, k, r << 2)?;
                            self.set_entry(1, k, g << 2)?;
                            self.set_entry(2, k, b << 2)?;
                            k += 1;
                        }
                    }
                }
            }
            return Ok(());
        }
        // two arrays of n boxes (`__builtin_vec_new`), used in turn
        let cap = n as usize;
        if cap > (1 << 24) {
            return Err(Fault("a colour count beyond what the allocator gives: the original writes through a null pointer"));
        }
        let mut boxes: [Vec<Region>; 2] = [vec![[0u8; 6]; cap], vec![[0u8; 6]; cap]];
        let put = |boxes: &mut [Vec<Region>; 2], side: usize, i: u32, b: Region| -> Result<(), Fault> {
            *boxes[side].get_mut(i as usize).ok_or(Fault("a box beyond the array: the original writes past it"))? = b;
            Ok(())
        };
        put(&mut boxes, 0, 0, bounding_box(hist, 0, 63, 0, 63, 0, 63)?)?;
        let mut cur = 0usize; // local_26
        let mut count = 1u32; // local_20: boxes in the current array
        if 1 < n {
            loop {
                let mut made = 0u32; // local_24: boxes in the next array
                let mut left = count; // local_30: boxes of the current array not yet handled
                let mut k = 0u32; // local_2c
                let nxt = cur ^ 1;
                if count != 0 && n != 0 {
                    loop {
                        let b = boxes[cur][k as usize];
                        let size = [b[1].wrapping_sub(b[0]), b[3].wrapping_sub(b[2]), b[5].wrapping_sub(b[4])];
                        if size[0] as u32 + size[1] as u32 + size[2] as u32 != 0 {
                            let [mr, mg, mb] = median_box(hist, b[0], b[1], b[2], b[3], b[4], b[5])?;
                            let halves = if mr == mg && mr == mb {
                                // the blue axis (with red's offset for the first half and
                                // blue's for the second: they are equal)
                                (bounding_box(hist, b[0], b[1], b[2], b[3], b[4], mr.wrapping_add(b[4]))?, bounding_box(hist, b[0], b[1], b[2], b[3], b[4].wrapping_add(mb), b[5])?)
                            } else if mg < mr && mb <= mr {
                                (bounding_box(hist, b[0], mr.wrapping_add(b[0]), b[2], b[3], b[4], b[5])?, bounding_box(hist, b[0].wrapping_add(mr), b[1], b[2], b[3], b[4], b[5])?)
                            } else if mr <= mg && mb < mg {
                                (bounding_box(hist, b[0], b[1], b[2], mg.wrapping_add(b[2]), b[4], b[5])?, bounding_box(hist, b[0], b[1], b[2].wrapping_add(mg), b[3], b[4], b[5])?)
                            } else {
                                // mg <= mb && mr < mb: the only case left
                                (bounding_box(hist, b[0], b[1], b[2], b[3], b[4], mb.wrapping_add(b[4]))?, bounding_box(hist, b[0], b[1], b[2], b[3], b[4].wrapping_add(mb), b[5])?)
                            };
                            put(&mut boxes, nxt, made, halves.0)?;
                            put(&mut boxes, nxt, made + 1, halves.1)?;
                            made += 2;
                            left -= 1;
                            if left + made >= n {
                                // no room for more: the boxes not yet handled go over as they
                                // are, up to n
                                let mut from = count - left;
                                while made < n {
                                    let b = boxes[cur][from as usize];
                                    put(&mut boxes, nxt, made, b)?;
                                    made += 1;
                                    from += 1;
                                }
                                count = made;
                                break;
                            }
                        } else {
                            put(&mut boxes, nxt, made, b)?;
                            made += 1;
                            left -= 1;
                        }
                        k += 1;
                        if !(k < count && made < n) {
                            break;
                        }
                    }
                }
                cur ^= 1;
                if count == made {
                    break;
                }
                count = made;
                if made >= n {
                    break;
                }
            }
        }
        for k in 0..count {
            let b = boxes[cur][k as usize];
            let size = [b[1].wrapping_sub(b[0]), b[3].wrapping_sub(b[2]), b[5].wrapping_sub(b[4])];
            let c = if size[0] as u32 + size[1] as u32 + size[2] as u32 == 0 {
                [b[0] << 2, b[2] << 2, b[4] << 2]
            } else {
                let m = color_ponderated_mean(hist, &b)?;
                let d = (k & 3) as u8;
                [(m[0] << 2).wrapping_add(d), (m[1] << 2).wrapping_add(d), (m[2] << 2).wrapping_add(d)]
            };
            for ch in 0..3 {
                self.set_entry(ch, k as i64, c[ch])?;
            }
        }
        Ok(())
    }

    /// The coarse-grid mapping of [`ImageMem::write_tile`] (remapping quality 1).
    fn remap(&mut self, x: i32, y: i32, w: i32, h: i32, src: &mut dyn ImageMem, sx: i32, sy: i32) -> Result<bool, Fault> {
        // 4, 8 or 16 cells an axis by the number of colours; the grid's records are laid out
        // for 16 whatever the number
        let (cells, shift) = if self.max_index > 0x100 {
            (16u32, 4u32)
        } else if self.max_index > 0x80 {
            (8, 5)
        } else {
            (4, 6)
        };
        let _ = cells;
        let mut grid: Vec<Vec<u16>> = vec![Vec::new(); 16 * 16 * 16];
        let cell = |r: u8, g: u8, b: u8| ((r >> shift) as usize) * 256 + ((g >> shift) as usize) * 16 + (b >> shift) as usize;
        for i in 0..self.max_index.max(0) as i64 {
            let c = cell(self.entry(0, i)?, self.entry(1, i)?, self.entry(2, i)?);
            // the count is a 16-bit word
            if grid[c].len() == 0xffff {
                return Err(Fault("more than 65,535 colours in one cell: the original's count wraps"));
            }
            grid[c].push(i as u16);
        }
        let (cw, ch, whole) = tile_clip(&self.image, x, y, w, h);
        let mut best = None::<u16>; // local_8020, kept from pixel to pixel
        for j in 0..ch {
            for i in 0..cw {
                let [r, g, b, _] = src.get_pixel(sx + i, sy + j)?;
                let (r, g, b) = (r & 0xfc, g & 0xfc, b & 0xfc);
                let list = &grid[cell(r, g, b)];
                if list.is_empty() {
                    self.set_pixel(x + i, y + j, [r, g, b, 0])?;
                    continue;
                }
                let mut least = 2147483648.0f32;
                for &e in list {
                    let d = dist(r, g, b, self.entry(0, e as i64)?, self.entry(1, e as i64)?, self.entry(2, e as i64)?) as f32;
                    if d < 1.0 {
                        best = Some(e);
                        break;
                    }
                    if d < least {
                        best = Some(e);
                        least = d;
                    }
                }
                let s = self.slot(x + i, y + j)?;
                self.indices.as_mut().unwrap()[s] = best.unwrap_or(0);
            }
        }
        Ok(whole)
    }
}

fn dist(r: u8, g: u8, b: u8, er: u8, eg: u8, eb: u8) -> i32 {
    let (dr, dg, db) = (r as i32 - er as i32, g as i32 - eg as i32, b as i32 - eb as i32);
    dr * dr + dg * dg + db * db
}

fn cmap_raw_offset(img: &ImageCmap, x: i32, y: i32, stride: i32, n: usize) -> Result<usize, Fault> {
    let buf = img.indices.as_ref().ok_or_else(fault_null)?;
    let i = y as i64 * stride as i64 + x as i64;
    if i < 0 || i as usize + n > buf.len() {
        return Err(fault_outside());
    }
    Ok(i as usize)
}

impl ImageBase for ImageCmap {
    fn image(&self) -> &Image {
        &self.image
    }
    fn image_mut(&mut self) -> &mut Image {
        &mut self.image
    }
}

impl ImageMem for ImageCmap {
    fn buffer(&self) -> Buffer<'_> {
        match &self.indices {
            Some(p) => Buffer::Words(p),
            None => Buffer::None,
        }
    }

    /// [soft 0x0861aa1c SI_ImageCMAP::setSize]: as `SI_ImageABGR`'s, two bytes a pixel.
    fn set_size(&mut self, width: i32, height: i32) {
        if width < 1 || height < 1 {
            self.indices = None;
        } else {
            let n = buffer_len(width, height, 1);
            self.indices.get_or_insert_with(Vec::new).resize(n, 0);
        }
        self.set_width(width);
        self.set_height(height);
    }

    /// [soft 0x0861b3fc SI_ImageCMAP::getPixel(uchar)]: the pixel's colour from the tables.
    fn get_pixel(&self, x: i32, y: i32) -> Result<[u8; 4], Fault> {
        let s = self.slot(x, y)?;
        let i = self.indices.as_ref().unwrap()[s] as i64;
        Ok([self.entry(0, i)?, self.entry(1, i)?, self.entry(2, i)?, self.entry(3, i)?])
    }

    /// [soft 0x0861b464 SI_ImageCMAP::setPixel(uchar)]: the nearest of the first
    /// `max_index` colours to the colour with the two low bits of each sample cleared
    /// (squared distance, compared as a float; the first of equals; an exact match ends the
    /// search). Alpha is not looked at. With no colour to search the index is a stack word
    /// nobody set (`Fault`).
    fn set_pixel(&mut self, x: i32, y: i32, v: [u8; 4]) -> Result<(), Fault> {
        let (r, g, b) = (v[0] & 0xfc, v[1] & 0xfc, v[2] & 0xfc);
        let mut best = None;
        let mut least = 2147483648.0f32;
        let mut i = 0i64;
        while i < self.max_index as i64 {
            let d = dist(r, g, b, self.entry(0, i)?, self.entry(1, i)?, self.entry(2, i)?) as f32;
            if d < 1.0 {
                best = Some(i);
                break;
            }
            if d < least {
                best = Some(i);
                least = d;
            }
            i += 1;
        }
        let best = best.ok_or(Fault("no colour to search: the original stores a stack word nobody set"))?;
        let s = self.slot(x, y)?;
        self.indices.as_mut().unwrap()[s] = best as u16;
        Ok(())
    }

    /// [soft 0x086436a8 SI_ImageCMAP::getPixel(ushort)]: `(int)(v / 255.0 × 65535.0)` in the
    /// x87 unit.
    fn get_pixel16(&self, x: i32, y: i32) -> Result<[u16; 4], Fault> {
        Ok(self.get_pixel(x, y)?.map(|v| trunc(F80::from(v as i32) / 255.0f64 * 65535.0f64) as u16))
    }

    /// [soft 0x086437d8 SI_ImageCMAP::setPixel(ushort)]: `(int)(v / 65535.0 × 255.0)`, its
    /// low byte, then the byte setter.
    fn set_pixel16(&mut self, x: i32, y: i32, v: [u16; 4]) -> Result<(), Fault> {
        self.set_pixel(x, y, v.map(|c| trunc(F80::from(c as i32) / 65535.0f64 * 255.0f64) as u8))
    }

    /// [soft 0x086438d0 SI_ImageCMAP::getPixel(float)]
    fn get_pixel_f(&self, x: i32, y: i32) -> Result<[f32; 4], Fault> {
        Ok(self.get_pixel(x, y)?.map(byte_to_float))
    }

    /// [soft 0x08643974 SI_ImageCMAP::setPixel(float)]
    fn set_pixel_f(&mut self, x: i32, y: i32, v: [f32; 4]) -> Result<(), Fault> {
        self.set_pixel(x, y, v.map(float_to_byte))
    }

    /// [soft 0x08643a40 SI_ImageCMAP::getPixel(uchar *, …, int)]
    fn get_pixel_as(&self, x: i32, y: i32, kind: i32) -> Result<Option<Channels>, Fault> {
        typed_get(self, x, y, kind)
    }

    /// [soft 0x08643ae0 SI_ImageCMAP::setPixel(uchar *, …, int)]: an absent channel is 0,
    /// alpha too.
    fn set_pixel_as(&mut self, x: i32, y: i32, v: &Channels, kind: i32) -> Result<(), Fault> {
        typed_set(self, x, y, v, kind, false)
    }

    /// [soft 0x0861aa90 SI_ImageCMAP::readTile(…, SI_ImageMem *, …)]: colours from this
    /// image into `dst` through the byte accessors, clipped to this image.
    fn read_tile(&mut self, x: i32, y: i32, w: i32, h: i32, dst: &mut dyn ImageMem, dx: i32, dy: i32) -> Result<bool, Fault> {
        let (w, h, whole) = tile_clip(&self.image, x, y, w, h);
        for j in 0..h {
            for i in 0..w {
                let v = self.get_pixel(x + i, y + j)?;
                dst.set_pixel(dx + i, dy + j, v)?;
            }
        }
        Ok(whole)
    }

    /// [soft 0x0861ab7c SI_ImageCMAP::writeTile(…, SI_ImageMem *, …)]: maps the pixels of
    /// `src` to indices of this image. Quality 0: each through [`ImageMem::set_pixel`].
    /// Quality 1: the colours are first sorted into a grid of 4, 8 or 16 cells an axis (more
    /// than 0x80, more than 0x100 colours), by their samples shifted right by 6, 5 or 4; a
    /// pixel (its samples with the two low bits cleared) takes the nearest colour **of its
    /// own cell** (as `setPixel`'s search), or, if its cell holds none, goes through
    /// `setPixel` and its search of every colour. Another quality: nothing, and the answer is
    /// true. Otherwise the answer is whether the tile was not clipped.
    fn write_tile(&mut self, x: i32, y: i32, w: i32, h: i32, src: &mut dyn ImageMem, sx: i32, sy: i32) -> Result<bool, Fault> {
        match self.remap_quality {
            0 => {
                let (w, h, whole) = tile_clip(&self.image, x, y, w, h);
                for j in 0..h {
                    for i in 0..w {
                        let v = src.get_pixel(sx + i, sy + j)?;
                        self.set_pixel(x + i, y + j, v)?;
                    }
                }
                Ok(whole)
            }
            1 => self.remap(x, y, w, h, src, sx, sy),
            _ => Ok(true),
        }
    }

    /// [soft 0x0861b53c SI_ImageCMAP::getCPixel]
    fn get_cpixel(&self, x: i32, y: i32) -> Result<i32, Fault> {
        let s = self.slot(x, y)?;
        Ok(self.indices.as_ref().unwrap()[s] as i32)
    }

    /// [soft 0x0861b55c SI_ImageCMAP::setCPixel]: the low 16 bits.
    fn set_cpixel(&mut self, x: i32, y: i32, v: i32) -> Result<(), Fault> {
        let s = self.slot(x, y)?;
        self.indices.as_mut().unwrap()[s] = v as u16;
        Ok(())
    }

    /// [soft 0x0861b580 SI_ImageCMAP::getColorIndexValue(int, uchar *, uchar *, uchar *)]
    fn get_color_index_value3(&self, i: i32) -> Result<Option<[u8; 3]>, Fault> {
        let i = i as i64;
        Ok(Some([self.entry(0, i)?, self.entry(1, i)?, self.entry(2, i)?]))
    }

    /// [soft 0x0861b614 SI_ImageCMAP::setColorIndexValue(int, uchar, uchar, uchar)]
    fn set_color_index_value3(&mut self, i: i32, v: [u8; 3]) -> Result<(), Fault> {
        for c in 0..3 {
            self.set_entry(c, i as i64, v[c])?;
        }
        Ok(())
    }

    /// [soft 0x0861b5c0 SI_ImageCMAP::getColorIndexValue(int, uchar *, uchar *, uchar *,
    /// uchar *)]
    fn get_color_index_value4(&self, i: i32) -> Result<Option<[u8; 4]>, Fault> {
        let i = i as i64;
        Ok(Some([self.entry(0, i)?, self.entry(1, i)?, self.entry(2, i)?, self.entry(3, i)?]))
    }

    /// [soft 0x0861b648 SI_ImageCMAP::setColorIndexValue(int, uchar, uchar, uchar, uchar)]
    fn set_color_index_value4(&mut self, i: i32, v: [u8; 4]) -> Result<(), Fault> {
        for c in 0..4 {
            self.set_entry(c, i as i64, v[c])?;
        }
        Ok(())
    }

    /// [soft 0x0861b68c SI_ImageCMAP::copyColorMap(first, last, to, dst)]: entries `first`
    /// to `last` (limited to this image's greatest index, and so that they fit below `dst`'s)
    /// into `dst` from entry `to` on, through its four-channel setter. Nothing when `first`
    /// is beyond this image's greatest index or `to` beyond `dst`'s.
    fn copy_color_map(&self, first: i32, last: i32, to: i32, dst: &mut dyn ImageMem) -> Result<(), Fault> {
        let mut last = last;
        if first > self.max_index {
            return Ok(());
        }
        if self.max_index < last {
            last = self.max_index;
        }
        if to > dst.index_max_value() {
            return Ok(());
        }
        let end = to.wrapping_add(last.wrapping_sub(first));
        if dst.index_max_value() < end {
            last = last.wrapping_sub(end.wrapping_sub(dst.index_max_value()));
        }
        let (mut i, mut t) = (first, to);
        while i <= last {
            let ii = i as i64;
            let v = [self.entry(0, ii)?, self.entry(1, ii)?, self.entry(2, ii)?, self.entry(3, ii)?];
            dst.set_color_index_value4(t, v)?;
            t = t.wrapping_add(1);
            i += 1;
        }
        Ok(())
    }

    /// [soft 0x0861b750 SI_ImageCMAP::generateColorIndex]: the colours of this image by
    /// median cut of the pixels of `src`, in a histogram of 64 levels a channel (each sample
    /// shifted right by 2). If fewer cells are occupied than the greatest index, each occupied
    /// cell is a colour (cell × 4), in the order red, green, blue. Otherwise boxes are cut
    /// in passes, starting from the box around every occupied cell: in a pass each box of
    /// more than one cell is cut in two at the mean place of its occupied cells (along the
    /// axis where that mean lies furthest from the box's lower bound, blue on a tie of all
    /// three; each half shrunk to its occupied cells; the two halves share the cut plane),
    /// a box of one cell is kept; when the next cut would make more boxes than the greatest
    /// index, the boxes not yet cut are kept as they are; passes end when no box was cut or
    /// there are as many boxes as the greatest index. A box of one cell gives that cell × 4,
    /// another the mean of its cells weighted by their counts, × 4, plus the box's number
    /// modulo 4 in each sample. Alpha is not set; the table's last entry stays as it was.
    fn generate_color_index(&mut self, src: &dyn ImageMem) -> Result<(), Fault> {
        let mut hist = Histogram(vec![0u32; 64 * 64 * 64]);
        let (w, h) = (src.width(), src.height());
        for x in 0..w {
            for y in 0..h {
                let [r, g, b, _] = src.get_pixel(x, y)?;
                let i = (r >> 2) as usize * 4096 + (g >> 2) as usize * 64 + (b >> 2) as usize;
                hist.0[i] = hist.0[i].wrapping_add(1);
            }
        }
        self.median_cut(&hist)
    }

    /// [soft 0x08643688 SI_ImageCMAP::getIndexMaxValue]
    fn index_max_value(&self) -> i32 {
        self.max_index
    }

    /// [soft 0x08643698 SI_ImageCMAP::setIndexMaxValue]
    fn set_index_max_value(&mut self, v: i32) {
        self.max_index = v;
    }
}

// ---------------------------------------------------------------------------------------------
// SI_ImageFile

/// The fields `SI_ImageFile` adds to `SI_Image` (0x18 to 0x34).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageFileFields {
    pub image: Image,
    pub planar_config: i32,
    pub compression: i32,
    /// 1: lines are read and written first to last; 0: last to first; another value: not at
    /// all (the whole-image routines answer 1 at once).
    pub orientation: i32,
    pub description: Option<Vec<u8>>,
    pub file_name: Option<Vec<u8>>,
    /// A pointer the plug-in sets to its format's name (not a copy).
    pub format_name: Option<Vec<u8>>,
    pub number_of_images: i32,
    pub current_image: i32,
}

impl Default for ImageFileFields {
    fn default() -> Self {
        ImageFileFields::new()
    }
}

impl ImageFileFields {
    /// [soft 0x0861bf70 SI_ImageFile::SI_ImageFile]: planar configuration 0, compression 0,
    /// orientation 1, no description, name or format name, one image, image 0.
    /// [soft 0x0861bfcc SI_ImageFile::~SI_ImageFile] frees the name and the description: the
    /// value going out of scope.
    pub fn new() -> ImageFileFields {
        ImageFileFields {
            image: Image::new(),
            planar_config: 0,
            compression: 0,
            orientation: 1,
            description: None,
            file_name: None,
            format_name: None,
            number_of_images: 1,
            current_image: 0,
        }
    }
}

/// The stream an image file is opened on (`openImage(FILE *, mode)`, slot 0x48).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdin,
    Stdout,
}

/// An image file: `SI_ImageFile`'s virtual methods (slots 0x24 to 0x68). The provided
/// methods are `SI_ImageFile`'s; the opening, closing, header and line methods are pure
/// virtual there, each format plug-in's own.
pub trait ImageFile: ImageBase {
    fn fields(&self) -> &ImageFileFields;
    fn fields_mut(&mut self) -> &mut ImageFileFields;

    /// Slot 0x24 [soft 0x08643c88 SI_ImageFile::setPlanarConfig]: stores, answers 1.
    fn set_planar_config(&mut self, v: i32) -> i32 {
        self.fields_mut().planar_config = v;
        1
    }
    /// Slot 0x28 [soft 0x08643ca0 SI_ImageFile::setCompression]: stores, answers 1.
    fn set_compression(&mut self, v: i32) -> i32 {
        self.fields_mut().compression = v;
        1
    }
    /// Slot 0x2c [soft 0x08643d18 SI_ImageFile::setNumberOfImages]
    fn set_number_of_images(&mut self, v: i32) {
        self.fields_mut().number_of_images = v;
    }
    /// Slot 0x30 [soft 0x08643d30 SI_ImageFile::setCurrentImage]: only up to the number of
    /// images (no lower bound); answers whether it was taken.
    fn set_current_image(&mut self, v: i32) -> bool {
        let ok = v <= self.fields().number_of_images;
        if ok {
            self.fields_mut().current_image = v;
        }
        ok
    }
    /// Slot 0x34 [soft 0x08643d70 SI_ImageFile::getColorIndexValue]: writes nothing.
    fn get_color_index_value(&self, _i: i32) -> Option<[u8; 3]> {
        None
    }
    /// Slot 0x38 [soft 0x08643d78 SI_ImageFile::setColorIndexValue]: nothing.
    fn set_color_index_value(&mut self, _i: i32, _v: [u8; 3]) {}
    /// Slot 0x3c [soft 0x08643d80 SI_ImageFile::getUsedColors]: 0.
    fn used_colors(&self) -> i32 {
        0
    }
    /// Slot 0x40 [soft 0x08643d8c SI_ImageFile::getIndexMaxValue]: 0.
    fn index_max_value(&self) -> i32 {
        0
    }
    /// Slot 0x44 [soft 0x08643d98 SI_ImageFile::setIndexMaxValue]: nothing.
    fn set_index_max_value(&mut self, _v: i32) {}
    /// Slot 0x48: opens on a standard stream.
    fn open_stream(&mut self, stream: Stream, mode: &[u8]) -> bool;
    /// Slot 0x4c: opens the named file.
    fn open_path(&mut self, name: &[u8], mode: &[u8]) -> bool;
    /// Slot 0x50.
    fn close(&mut self) -> bool;
    /// Slot 0x5c.
    fn read_header(&mut self) -> bool;
    /// Slot 0x60.
    fn write_header(&mut self) -> bool;
    /// Slot 0x64: line `line` of the file into `mem`.
    fn read_next_line(&mut self, mem: &mut dyn ImageMem, line: i32) -> Result<bool, Fault>;
    /// Slot 0x68: line `line` of `mem` to the file.
    fn write_next_line(&mut self, mem: &dyn ImageMem, line: i32) -> Result<bool, Fault>;

    /// Slot 0x54 [soft 0x0861c028 SI_ImageFile::readImage]: refuses (0) an image of another
    /// size; otherwise reads every line by orientation (1: first to last, 0: last to first,
    /// stopping with 0 at a line that fails; another orientation: nothing, 1).
    fn read_image(&mut self, mem: &mut dyn ImageMem) -> Result<bool, Fault> {
        if self.width() != mem.width() || self.height() != mem.height() {
            return Ok(false);
        }
        match self.fields().orientation {
            1 => {
                let mut line = 0;
                while line < self.height() {
                    if !self.read_next_line(mem, line)? {
                        return Ok(false);
                    }
                    line += 1;
                }
                Ok(true)
            }
            0 => {
                let mut line = self.height();
                loop {
                    line -= 1;
                    if line < 0 {
                        return Ok(true);
                    }
                    if !self.read_next_line(mem, line)? {
                        return Ok(false);
                    }
                }
            }
            _ => Ok(true),
        }
    }

    /// Slot 0x58 [soft 0x0861c0ac SI_ImageFile::writeImage]: the same for writing.
    fn write_image(&mut self, mem: &dyn ImageMem) -> Result<bool, Fault> {
        if self.width() != mem.width() || self.height() != mem.height() {
            return Ok(false);
        }
        match self.fields().orientation {
            1 => {
                let mut line = 0;
                while line < self.height() {
                    if !self.write_next_line(mem, line)? {
                        return Ok(false);
                    }
                    line += 1;
                }
                Ok(true)
            }
            0 => {
                let mut line = self.height();
                loop {
                    line -= 1;
                    if line < 0 {
                        return Ok(true);
                    }
                    if !self.write_next_line(mem, line)? {
                        return Ok(false);
                    }
                }
            }
            _ => Ok(true),
        }
    }

    /// [soft 0x08643da0 SI_ImageFile::readNextLine(SI_ImageMem *)]: line 0.
    fn read_line0(&mut self, mem: &mut dyn ImageMem) -> Result<bool, Fault> {
        self.read_next_line(mem, 0)
    }
    /// [soft 0x08643dc0 SI_ImageFile::writeNextLine(SI_ImageMem *)]: line 0.
    fn write_line0(&mut self, mem: &dyn ImageMem) -> Result<bool, Fault> {
        self.write_next_line(mem, 0)
    }
    /// [soft 0x0861c12c SI_ImageFile::setDescription]: a copy (up to its first zero), or
    /// none.
    fn set_description(&mut self, d: Option<&[u8]>) {
        self.fields_mut().description = d.map(c_str);
    }
    /// [soft 0x0861c170 SI_ImageFile::setFileName]: likewise.
    fn set_file_name(&mut self, d: Option<&[u8]>) {
        self.fields_mut().file_name = d.map(c_str);
    }
    /// [soft 0x08643de0 SI_ImageFile::setFormatName]
    fn set_format_name(&mut self, d: Option<&[u8]>) {
        self.fields_mut().format_name = d.map(|v| v.to_vec());
    }
    /// [soft 0x08643df0 SI_ImageFile::setOrientation]
    fn set_orientation(&mut self, v: i32) {
        self.fields_mut().orientation = v;
    }
    /// [soft 0x08643cb8 SI_ImageFile::getPlanarConfig]
    fn planar_config(&self) -> i32 {
        self.fields().planar_config
    }
    /// [soft 0x08643cc8 SI_ImageFile::getCompression]
    fn compression(&self) -> i32 {
        self.fields().compression
    }
    /// [soft 0x08643cd8 SI_ImageFile::getDescription]
    fn description(&self) -> Option<&[u8]> {
        self.fields().description.as_deref()
    }
    /// [soft 0x08643ce8 SI_ImageFile::getFileName]
    fn file_name(&self) -> Option<&[u8]> {
        self.fields().file_name.as_deref()
    }
    /// [soft 0x08643cf8 SI_ImageFile::getFormatName]
    fn format_name(&self) -> Option<&[u8]> {
        self.fields().format_name.as_deref()
    }
    /// [soft 0x08643d08 SI_ImageFile::getOrientation]
    fn orientation(&self) -> i32 {
        self.fields().orientation
    }
    /// [soft 0x08643d50 SI_ImageFile::getNumberOfImages]
    fn number_of_images(&self) -> i32 {
        self.fields().number_of_images
    }
    /// [soft 0x08643d60 SI_ImageFile::getCurrentImage]
    fn current_image(&self) -> i32 {
        self.fields().current_image
    }
}

/// A C string's bytes up to its first zero (`strdup`).
fn c_str(s: &[u8]) -> Vec<u8> {
    s.iter().take_while(|&&c| c != 0).copied().collect()
}

/// `strcasecmp(a, b) == 0` on C strings.
fn same_name(a: &[u8], b: &[u8]) -> bool {
    let (a, b) = (c_str(a), c_str(b));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------------------------
// SI_ImageFileFormat

/// A format plug-in's virtual methods (`SI_ImageFileFormat`, method table at 0xc).
pub trait FormatImpl {
    /// Slot 0xc [soft 0x08643e48 SI_ImageFileFormat::createImage]: none.
    fn create_image(&self) -> Option<Box<dyn ImageFile>> {
        None
    }
    /// Slot 0x10 [soft 0x08643e54 SI_ImageFileFormat::checkSignature]: 0. The argument is
    /// the file's first 1,024 bytes (zeros after its end).
    fn check_signature(&self, _head: &[u8; 1024]) -> i16 {
        0
    }
    /// Slot 0x14 [soft 0x08643e60 SI_ImageFileFormat::haveSignature]: 1.
    fn have_signature(&self) -> i16 {
        1
    }
    /// Slot 0x18 [soft 0x08643e6c SI_ImageFileFormat::writeEnabled]: 1.
    fn write_enabled(&self) -> i16 {
        1
    }
}

/// `SI_ImageFileFormat` (0x10 bytes): its name, its extensions (the first is the primary
/// one), and the plug-in's methods.
pub struct FileFormat {
    pub name: Option<Vec<u8>>,
    pub extensions: Vec<Vec<u8>>,
    pub imp: Box<dyn FormatImpl>,
}

impl FileFormat {
    /// [soft 0x08643e2c SI_ImageFileFormat::getName]
    pub fn name(&self) -> Option<&[u8]> {
        self.name.as_deref()
    }
    /// [soft 0x08643e38 SI_ImageFileFormat::getPrimaryExtension]: the first extension (a
    /// format the list refused has none: the original reads through a null pointer).
    pub fn primary_extension(&self) -> Result<&[u8], Fault> {
        self.extensions.first().map(|v| v.as_slice()).ok_or(Fault("a format without extensions: the original reads through a null pointer"))
    }
    /// [soft 0x0861c360 SI_ImageFileFormat::addExtension]
    pub fn add_extension(&mut self, ext: &[u8]) {
        self.extensions.push(c_str(ext));
    }
}

/// The class's list of formats (`formatList`, `formatCounter`), in the order they were made.
#[derive(Default)]
pub struct FormatRegistry {
    pub formats: Vec<FileFormat>,
}

impl FormatRegistry {
    /// [soft 0x0861c754 SI_ImageFileFormat::init]: nothing.
    pub fn init(&mut self) {}

    /// [soft 0x0861c1b4 SI_ImageFileFormat::SI_ImageFileFormat(name, extension)]: a format
    /// of a name not yet in the list is added with its one extension and its place returned;
    /// otherwise the object is left with no name and no extensions and not listed (`None`).
    pub fn register(&mut self, name: &[u8], ext: &[u8], imp: Box<dyn FormatImpl>) -> Option<usize> {
        if self.find_by_name(name).is_some() {
            return None;
        }
        self.formats.push(FileFormat { name: Some(c_str(name)), extensions: vec![c_str(ext)], imp });
        Some(self.formats.len() - 1)
    }

    /// [soft 0x0861c260 SI_ImageFileFormat::~SI_ImageFileFormat]: takes the format out of the
    /// list (the list's block is freed when it empties); its strings are freed with it.
    pub fn unregister(&mut self, index: usize) -> Option<FileFormat> {
        (index < self.formats.len()).then(|| self.formats.remove(index))
    }

    /// [soft 0x0861c3a8 SI_ImageFileFormat::findBySignature]: the first format that has a
    /// signature and whose check takes the bytes.
    pub fn find_by_signature(&self, head: &[u8; 1024]) -> Option<usize> {
        self.formats.iter().position(|f| f.imp.have_signature() != 0 && f.imp.check_signature(head) != 0)
    }

    /// [soft 0x0861c41c SI_ImageFileFormat::findByName]: by name, case ignored.
    pub fn find_by_name(&self, name: &[u8]) -> Option<usize> {
        self.formats.iter().position(|f| f.name.as_deref().is_some_and(|n| same_name(name, n)))
    }

    /// [soft 0x0861c46c SI_ImageFileFormat::findByExtension]: by what follows the last dot
    /// of the name (nothing when there is no dot), case ignored. The extension is copied
    /// into 128 bytes: a longer one overruns the original's stack.
    pub fn find_by_extension(&self, name: &[u8]) -> Result<Option<usize>, Fault> {
        let ext = extension(name)?;
        Ok(self.formats.iter().position(|f| f.extensions.iter().any(|e| same_name(&ext, e))))
    }

    /// [soft 0x0861c510 SI_ImageFileFormat::findByIndex]
    pub fn find_by_index(&self, i: i32) -> Option<usize> {
        (i >= 0 && (i as usize) < self.formats.len()).then_some(i as usize)
    }

    /// [soft 0x0861c534 SI_ImageFileFormat::whichFormat(name, format name)]: the format of an
    /// existing file. `head` is what `fopen(name, "rb")` (standard input for `-`) and a read
    /// of up to 1,024 bytes give: `None` if the file does not open; an empty file is no
    /// format. A format with a signature is found only by it; one without, by the format
    /// name given or by the name's extension (none for `-`). Without a format name, a name
    /// without extension is no format.
    pub fn which_format(&self, name: &[u8], format: Option<&[u8]>, head: Option<&[u8]>) -> Result<Option<usize>, Fault> {
        let Some(head) = head else { return Ok(None) };
        if head.is_empty() {
            return Ok(None);
        }
        let mut buf = [0u8; 1024];
        let n = head.len().min(1024);
        buf[..n].copy_from_slice(&head[..n]);
        if let Some(i) = self.find_by_signature(&buf) {
            return Ok(Some(i));
        }
        let ext = if c_str(name) == b"-" { Vec::new() } else { extension(name)? };
        if format.is_none() && ext.is_empty() {
            return Ok(None);
        }
        for (i, f) in self.formats.iter().enumerate() {
            if f.imp.have_signature() != 0 {
                continue;
            }
            if let (Some(fname), Some(n)) = (format, f.name.as_deref()) {
                if same_name(fname, n) {
                    return Ok(Some(i));
                }
            }
            if !ext.is_empty() && f.extensions.iter().any(|e| same_name(&ext, e)) {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }
}

/// What follows the last dot of a name (`strrchr` and `strcpy` into 128 bytes).
fn extension(name: &[u8]) -> Result<Vec<u8>, Fault> {
    let name = c_str(name);
    let ext = match name.iter().rposition(|&c| c == b'.') {
        Some(p) => name[p + 1..].to_vec(),
        None => Vec::new(),
    };
    if ext.len() >= 128 {
        return Err(Fault("an extension of 128 bytes or more: the original overruns its stack"));
    }
    Ok(ext)
}

// ---------------------------------------------------------------------------------------------
// SI_ImageProc, SI_ImageProcType

/// A processor plug-in's object (`SI_ImageProc`, method table at 0).
/// [soft 0x0861ce80 SI_ImageProc::SI_ImageProc] and [soft 0x0861ce90 SI_ImageProc::~SI_ImageProc]
/// only set the method table and delete the object.
pub trait ImageProc {
    /// Slot 0xc: pure virtual (the processing).
    fn process(&mut self, src: &mut dyn ImageMem, dst: &mut dyn ImageMem) -> Result<bool, Fault>;
    /// Slot 0x10 [soft 0x08644100 SI_ImageProc::readParam]: 0 (no parameter taken).
    fn read_param(&mut self, _text: &[u8]) -> i32 {
        0
    }
}

/// A processor plug-in's type (`SI_ImageProcType`, method table at 4).
pub trait ProcTypeImpl {
    /// Slot 0xc: pure virtual (makes the processor).
    fn create_proc(&self) -> Option<Box<dyn ImageProc>>;
    /// Slot 0x10 [soft 0x08644144 SI_ImageProcType::getParamDescription]: an empty text.
    fn param_description(&self) -> &[u8] {
        b""
    }
}

/// `SI_ImageProcType`: a name and the plug-in's methods.
pub struct ProcType {
    pub name: Option<Vec<u8>>,
    pub imp: Box<dyn ProcTypeImpl>,
}

impl ProcType {
    /// [soft 0x08644138 SI_ImageProcType::getName]
    pub fn name(&self) -> Option<&[u8]> {
        self.name.as_deref()
    }
}

/// The class's list of processor types (`procList`, `procCount`).
#[derive(Default)]
pub struct ProcRegistry {
    pub types: Vec<ProcType>,
}

impl ProcRegistry {
    /// [soft 0x0861cfe0 SI_ImageProcType::init]: nothing.
    pub fn init(&mut self) {}

    /// [soft 0x0861ceb4 SI_ImageProcType::SI_ImageProcType(name)]: added under a copy of its
    /// name unless the name is listed already (then left nameless and not listed).
    /// [soft 0x0861cf28 SI_ImageProcType::~SI_ImageProcType] frees the name but **does not
    /// take the type out of the list**; the list keeps a pointer to the freed object
    /// (`retire` here empties the name and keeps the entry).
    pub fn register(&mut self, name: &[u8], imp: Box<dyn ProcTypeImpl>) -> Option<usize> {
        if self.find_by_name(name).is_some() {
            return None;
        }
        self.types.push(ProcType { name: Some(c_str(name)), imp });
        Some(self.types.len() - 1)
    }

    /// See [`ProcRegistry::register`]: the destructor's effect on a listed type.
    pub fn retire(&mut self, index: usize) {
        if let Some(t) = self.types.get_mut(index) {
            t.name = None;
        }
    }

    /// [soft 0x0861cf68 SI_ImageProcType::findByName]: case ignored. A retired type's name
    /// is freed memory in the original (`Fault`).
    pub fn find_by_name(&self, name: &[u8]) -> Option<usize> {
        self.types.iter().position(|t| t.name.as_deref().is_some_and(|n| same_name(name, n)))
    }

    /// [soft 0x0861cfbc SI_ImageProcType::findByIndex]
    pub fn find_by_index(&self, i: i32) -> Option<usize> {
        (i >= 0 && (i as usize) < self.types.len()).then_some(i as usize)
    }
}

// ---------------------------------------------------------------------------------------------
// ilImageUtil.c++

/// What became of one shared object `SI_ImageInit` tried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginAttempt {
    /// `<directory>/<file>`, as given to `dlopen` (lazy binding).
    pub path: Vec<u8>,
    /// `SI_` + the file name from its third character to its first dot + `register`.
    pub symbol: Vec<u8>,
    pub outcome: PluginOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginOutcome {
    /// `dlopen` failed.
    NotLoaded,
    /// No such symbol: the object is closed again.
    NoSymbol,
    /// The registration routine was called.
    Registered,
}

/// The run-time state of the image library: the two class lists and `SI_ImageInit`'s flag.
#[derive(Default)]
pub struct ImageLibrary {
    pub formats: FormatRegistry,
    pub procs: ProcRegistry,
    pub done: bool,
}

/// The directory where `SI_ImageInit` looks for plug-ins without `SI_IMAGE_PATH` and
/// `SI_LOCATION`.
pub const DEFAULT_IMAGE_PATH: &[u8] = b"/usr/softimage/3D/dso/sil";

impl ImageLibrary {
    /// [soft 0x0861c990 SI_ImageInit]: once only. The search path is `SI_IMAGE_PATH`, or
    /// `SI_LOCATION` + `/3D/dso/sil`, or `/usr/softimage/3D/dso/sil`; its directories are
    /// separated by colons (empty ones skipped, as `strtok` does). In each directory that
    /// opens, in the order the directory lists them, every entry other than `.` and `..`
    /// whose text after its **first** dot is exactly `so` is opened with `dlopen`, and the
    /// symbol `SI_<name from its third character up to its first dot>register` (for
    /// `ilImageSOFT.so`, `SI_ImageSOFTregister`) is called if present, the object closed
    /// otherwise. `list_dir` answers a directory's entries (`None`: it does not open);
    /// `load` stands for `dlopen`, `dlsym` and the call (its answer is what happened).
    /// Answers the attempts in order (none on a second call). The original first calls
    /// [`swap_float`] on 1.0 and drops the answer.
    pub fn init(
        &mut self,
        image_path: Option<&[u8]>,
        location: Option<&[u8]>,
        mut list_dir: impl FnMut(&[u8]) -> Option<Vec<Vec<u8>>>,
        mut load: impl FnMut(&mut ImageLibrary, &[u8], &[u8]) -> PluginOutcome,
    ) -> Result<Vec<PluginAttempt>, Fault> {
        let _ = swap_float(1.0);
        if self.done {
            return Ok(Vec::new());
        }
        self.done = true;
        self.formats.init();
        self.procs.init();
        let path = match (image_path, location) {
            (Some(p), _) => c_str(p),
            (None, Some(l)) => {
                let mut v = c_str(l);
                v.extend_from_slice(b"/3D/dso/sil");
                if v.len() >= 1024 {
                    return Err(Fault("a location of 1,013 bytes or more: the original overruns its stack"));
                }
                v
            }
            (None, None) => DEFAULT_IMAGE_PATH.to_vec(),
        };
        let mut out = Vec::new();
        for dir in path.split(|&c| c == b':').filter(|d| !d.is_empty()) {
            let Some(entries) = list_dir(dir) else { continue };
            for name in entries {
                let name = c_str(&name);
                if name == b"." || name == b".." {
                    continue;
                }
                let Some(dot) = name.iter().position(|&c| c == b'.') else { continue };
                if &name[dot + 1..] != b"so" {
                    continue;
                }
                let mut file = dir.to_vec();
                file.push(b'/');
                file.extend_from_slice(&name);
                if file.len() >= 1024 {
                    return Err(Fault("a plug-in path of 1,024 bytes or more: the original overruns its stack"));
                }
                let mut symbol = b"SI_".to_vec();
                symbol.extend_from_slice(name.get(2..).unwrap_or(&[]));
                if let Some(p) = symbol.iter().position(|&c| c == b'.') {
                    symbol.truncate(p);
                }
                symbol.extend_from_slice(b"register");
                if symbol.len() >= 128 {
                    return Err(Fault("a plug-in name of about 120 bytes or more: the original overruns its stack"));
                }
                let outcome = load(self, &file, &symbol);
                out.push(PluginAttempt { path: file, symbol, outcome });
            }
        }
        Ok(out)
    }

    /// [soft 0x0861cc6c SI_ImageOpenFile(name, format name)]: the format by
    /// [`FormatRegistry::which_format`] (`head`: the file's first bytes, see there), an image
    /// file made by it and opened for reading (`rb`; on standard input for `-`). `None` if
    /// there is no format, it makes no image, or the opening fails (the image is deleted).
    /// The original first calls `SI_ImageInit`; here the caller has.
    pub fn open_file(&self, name: &[u8], format: Option<&[u8]>, head: Option<&[u8]>) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        let Some(f) = self.formats.which_format(name, format, head)? else { return Ok(None) };
        let Some(mut img) = self.formats.formats[f].imp.create_image() else { return Ok(None) };
        let ok = if c_str(name) == b"-" { img.open_stream(Stream::Stdin, b"rb") } else { img.open_path(name, b"rb") };
        Ok(ok.then_some(img))
    }

    /// [soft 0x0861cd20 SI_ImageCreateFile(name, format name)]: the format by name if one is
    /// given and listed, else by the name's extension (not for `-`); `None` without one, if
    /// it is not write-enabled, makes no image, or the opening for writing (`wb`; standard
    /// output for `-`) fails.
    pub fn create_file(&self, name: &[u8], format: Option<&[u8]>) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        let mut f = format.and_then(|n| self.formats.find_by_name(n));
        if f.is_none() && c_str(name) != b"-" {
            f = self.formats.find_by_extension(name)?;
        }
        let Some(f) = f else { return Ok(None) };
        let fmt = &self.formats.formats[f];
        if fmt.imp.write_enabled() == 0 {
            return Ok(None);
        }
        let Some(mut img) = fmt.imp.create_image() else { return Ok(None) };
        let ok = if c_str(name) == b"-" { img.open_stream(Stream::Stdout, b"wb") } else { img.open_path(name, b"wb") };
        Ok(ok.then_some(img))
    }
}

// ---------------------------------------------------------------------------------------------
// quant.c: between the image classes and the program's pictures

/// [soft 0x085b3214 SI_ImageMem2RGBA]: an RGBA picture of the image's size (16-bit), ratio
/// 1.0, fields 0, the word at 10 set to 1, pixel (x, y) the image's pixel (x, y) (bytes red,
/// green, blue, alpha; no turning of rows), read last row first. `None` when the allocation
/// (`SI_calloc` of width × height four-byte pixels) gives nothing, which it does for an
/// area of 0: the original then answers 0 with the size already set.
pub fn mem_to_rgba(mem: &dyn ImageMem) -> Result<Result<PictureRgba, PictureRgba>, Fault> {
    let (w16, h16) = (mem.width() as i16, mem.height() as i16);
    let mut pic = PictureRgba { width: w16, height: h16, ratio: 1.0, fields: 0, pad: 1, data: Vec::new() };
    let n = w16 as i32 * h16 as i32;
    if n <= 0 {
        // SI_calloc returns null for a count of 0; a negative count fails in calloc
        return Ok(Err(pic));
    }
    pic.data = vec![0u8; n as usize * 4];
    let w = mem.width();
    let mut y = mem.height();
    loop {
        y -= 1;
        if y < 0 {
            break;
        }
        for x in 0..w {
            let v = mem.get_pixel(x, y)?;
            let i = (y as i64 * w as i64 + x as i64) * 4;
            if i < 0 || i as usize + 4 > pic.data.len() {
                return Err(Fault("the image is larger than its 16-bit size: the original writes past the picture"));
            }
            pic.data[i as usize..i as usize + 4].copy_from_slice(&v);
        }
    }
    Ok(Ok(pic))
}

/// [soft 0x085b332c SI_PictRGBA2ImageMem]: a new four-byte image of the picture's size, each
/// pixel set in the same place, last row first.
pub fn rgba_to_mem(pic: &PictureRgba) -> Result<ImageAbgr, Fault> {
    let mut img = ImageAbgr::new(pic.width as i32, pic.height as i32);
    let w = pic.width as i32;
    let mut y = pic.height as i32;
    loop {
        y -= 1;
        if y < 0 {
            break;
        }
        for x in 0..w {
            let i = (x as i64 + w as i64 * y as i64) as usize * 4;
            let p = pic.data.get(i..i + 4).ok_or(Fault("a picture shorter than its size: the original reads past it"))?;
            img.set_pixel(x, y, [p[0], p[1], p[2], p[3]])?;
        }
    }
    Ok(img)
}

/// [soft 0x085b3460 imageMem2DGT]: an index picture of the image's size with
/// `getIndexMaxValue() + 1` palette entries of 8 bits a channel; the indices taken **last
/// image row first** (so the index picture is upside down against the image), the palette
/// from the four-channel colour getter (an entry the getter leaves alone takes the values
/// the previous entry left in the variables; before the first, stack bytes: `Fault`).
pub fn mem_to_dgt(mem: &dyn ImageMem) -> Result<crate::dgt::DgPicture, Fault> {
    let colours = mem.index_max_value().wrapping_add(1);
    let mut d = crate::dgt::DgPicture::allocate(mem.width(), mem.height(), colours, &[8, 8, 8, 8]);
    let mut k = 0usize;
    let w = mem.width();
    let mut y = mem.height();
    loop {
        y -= 1;
        if y < 0 {
            break;
        }
        for x in 0..w {
            let v = mem.get_cpixel(x, y)?;
            let data = d.data.as_mut().ok_or(Fault("an index picture without pixels: the original writes through a null pointer"))?;
            *data.get_mut(k).ok_or(Fault("more pixels than the index picture holds"))? = v as u16;
            k += 1;
        }
    }
    let mut last: Option<[u8; 4]> = None;
    let mut i = 0i32;
    while i <= mem.index_max_value() {
        if let Some(v) = mem.get_color_index_value4(i)? {
            last = Some(v);
        }
        let v = last.ok_or(Fault("a colour getter that writes nothing: the original reads stack bytes"))?;
        let pal = d.palette.as_mut().ok_or(Fault("an index picture without a palette"))?;
        pal.set_entry(i as u32, v.map(|c| c as i32)).map_err(|_| Fault("a palette entry the original sets through a null pointer"))?;
        i += 1;
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abgr_stores_alpha_blue_green_red() {
        let mut img = ImageAbgr::new(2, 2);
        img.set_pixel(1, 1, [10, 20, 30, 40]).unwrap();
        assert_eq!(&img.pixels.as_ref().unwrap()[12..16], &[40, 30, 20, 10]);
        assert_eq!(img.get_pixel(1, 1).unwrap(), [10, 20, 30, 40]);
        assert_eq!(img.get_pixel16(1, 1).unwrap()[0], 10 * 257);
        assert!(img.get_pixel(2, 1).is_err());
        assert!(ImageAbgr::new(0, 3).get_pixel(0, 0).is_err());
    }

    #[test]
    fn float_samples_truncate() {
        let mut img = ImageAbgr::new(1, 1);
        img.set_pixel_f(0, 0, [0.999, 1.0, 0.5, 0.0]).unwrap();
        assert_eq!(img.get_pixel(0, 0).unwrap(), [254, 255, 127, 0]);
        let cm = ImageCmap::new(2, 1, 1);
        assert_eq!(cm.image.photometric, PHOTOMETRIC_CMAP);
    }

    #[test]
    fn few_colours_are_kept_as_cells() {
        let mut img = ImageAbgr::new(3, 1);
        for (x, c) in [[0u8, 0, 0, 255], [255, 0, 0, 255], [8, 9, 10, 255]].iter().enumerate() {
            img.set_pixel(x as i32, 0, *c).unwrap();
        }
        let mut cm = ImageCmap::new(256, 3, 1);
        cm.generate_color_index(&img).unwrap();
        // cells in red, green, blue order, × 4
        assert_eq!(cm.get_color_index_value3(0).unwrap().unwrap(), [0, 0, 0]);
        assert_eq!(cm.get_color_index_value3(1).unwrap().unwrap(), [8, 8, 8]);
        assert_eq!(cm.get_color_index_value3(2).unwrap().unwrap(), [252, 0, 0]);
        cm.write_tile(0, 0, 3, 1, &mut img, 0, 0).unwrap();
        assert_eq!([0, 1, 2].map(|x| cm.get_cpixel(x, 0).unwrap()), [0, 2, 1]);
    }

    #[test]
    fn median_cut_makes_the_greatest_index_of_colours() {
        let mut img = ImageAbgr::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                img.set_pixel(x, y, [(x * 4) as u8, (y * 4) as u8, ((x ^ y) * 4) as u8, 255]).unwrap();
            }
        }
        let mut cm = ImageCmap::new(16, 64, 64);
        cm.generate_color_index(&img).unwrap();
        cm.write_tile(0, 0, 64, 64, &mut img, 0, 0).unwrap();
        let mut used = [false; 16];
        for y in 0..64 {
            for x in 0..64 {
                used[cm.get_cpixel(x, y).unwrap() as usize] = true;
            }
        }
        // 15 colours made; the sixteenth entry stays black and unused
        assert!(!used[15]);
        assert!(used[..15].iter().filter(|&&u| u).count() > 8);
    }

    #[test]
    fn registry_finds_by_name_and_extension() {
        struct F;
        impl FormatImpl for F {
            fn have_signature(&self) -> i16 {
                0
            }
        }
        let mut r = FormatRegistry::default();
        assert_eq!(r.register(b"SOFT", b"pic", Box::new(F)), Some(0));
        assert_eq!(r.register(b"soft", b"x", Box::new(F)), None);
        r.formats[0].add_extension(b"PIC2");
        assert_eq!(r.find_by_name(b"Soft"), Some(0));
        assert_eq!(r.find_by_extension(b"a.b/c.pic2").unwrap(), Some(0));
        assert_eq!(r.which_format(b"c.pic", None, Some(b"x")).unwrap(), Some(0));
        assert_eq!(r.which_format(b"c", None, Some(b"x")).unwrap(), None);
        assert_eq!(r.which_format(b"c", Some(b"SOFT"), Some(b"x")).unwrap(), Some(0));
        assert_eq!(r.which_format(b"c.pic", None, Some(b"")).unwrap(), None);
    }

    #[test]
    fn init_loads_files_ending_in_so_after_the_first_dot() {
        let mut lib = ImageLibrary::default();
        let tried = lib
            .init(None, Some(b"/opt/si"), |d| (d == b"/opt/si/3D/dso/sil").then(|| vec![b".".to_vec(), b"ilImageSOFT.so".to_vec(), b"a.b.so".to_vec(), b"x.so.1".to_vec()]), |_, _, _| PluginOutcome::Registered)
            .unwrap();
        assert_eq!(tried.len(), 1);
        assert_eq!(tried[0].symbol, b"SI_ImageSOFTregister");
        assert!(lib.init(None, None, |_| None, |_, _, _| PluginOutcome::NotLoaded).unwrap().is_empty());
    }
}
