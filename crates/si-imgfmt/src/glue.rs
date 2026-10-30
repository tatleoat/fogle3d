// SPDX-License-Identifier: MIT
//! quant.c's four routines between the picture library and the program's pictures: the
//! picture loader, the two savers and the converter from an index picture to an image
//! (finding 0014, imgfmt2 section).
//!
//! They talk to the image classes of [`crate::image`] only through their virtual methods
//! and to the program through [`Glue`] and [`Env`]. The Rust follows the listings call for
//! call (the order of the virtual calls is kept). Compared with the original in the emulator
//! (finding 0014, img1 section): the small inline methods of the image classes lie above the
//! licence cut-off of the harness, so the check runs the original with recorded stand-ins for
//! the constructors, the open/create routines, the conversions and the object methods, and
//! replays that tape through [`Env`]. Object deletes are not part of the tape.

use crate::dgt::DgPicture;
use crate::image::{mem_to_dgt, mem_to_rgba, rgba_to_mem, ImageAbgr, ImageCmap, ImageFile, ImageLibrary, ImageMem};
use crate::tiff::Fault;
use si_picture::PictureRgba;
use si_x87::{Round, F80};

/// What the four routines call outside the image classes.
pub trait Glue {
    /// `SI_cursorChange` (0x083d1910): 5 while a file is read or written, 0 afterwards.
    fn cursor_change(&mut self, shape: i32);
    /// `SI_dialogRequest` (0x08141c00).
    fn request(&mut self, line1: &str, line2: Option<&str>, line3: Option<&str>, button: &str);
    /// `SI_getImageChoice` (0x085b4968, not ported): which image of a file with several the
    /// user wants.
    fn get_image_choice(&mut self, file: &dyn ImageFile) -> i32;
    /// The global `si_alphaOn` (0x086f81a4).
    fn alpha_on(&self) -> i16;
}

/// What the four routines call that lives outside them and is, in the original, a call into the
/// image library or a conversion: opening and creating files, the two image constructors and the
/// three conversions between images and pictures. [`RealEnv`] answers them with the ports; the
/// emulator check (`imgfmt-emucheck`, `glue.rs`) answers them from the recorded stand-ins of
/// `re/tools/img1/gen_glue.py`.
pub trait Env: Glue {
    /// `SI_ImageOpenFile` (0x0861cc6c) with no format name.
    fn open_file(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault>;
    /// `SI_ImageCreateFile` (0x0861cd20) with no format name.
    fn create_file(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault>;
    /// The constructor `SI_ImageCMAP::SI_ImageCMAP` (0x0861a8c0) of an object the caller `new`ed.
    fn new_cmap(&mut self, colours: i32, width: i32, height: i32) -> Box<dyn ImageMem>;
    /// The constructor `SI_ImageABGR::SI_ImageABGR` (0x0861a0b0).
    fn new_abgr(&mut self, width: i32, height: i32) -> Box<dyn ImageMem>;
    /// `SI_ImageMem2RGBA` (0x085b3214).
    fn mem_to_rgba(&mut self, mem: &dyn ImageMem) -> Result<PictureRgba, Fault>;
    /// `imageMem2DGT` (0x085b3460).
    fn mem_to_dgt(&mut self, mem: &dyn ImageMem) -> Result<DgPicture, Fault>;
    /// `SI_PictRGBA2ImageMem` (0x085b332c).
    fn rgba_to_mem(&mut self, pic: &PictureRgba) -> Result<Box<dyn ImageMem>, Fault>;
    /// `SI_dgPictureDispose` (0x085b9d34) on the index picture of an empty image; the port
    /// just drops it.
    fn dispose_dg(&mut self) {}
}

/// [`Env`] over the ports: the image library, the host and the conversions of `image.rs`.
pub struct RealEnv<'a> {
    pub lib: &'a ImageLibrary,
    pub host: &'a mut dyn Glue,
}

impl Glue for RealEnv<'_> {
    fn cursor_change(&mut self, shape: i32) {
        self.host.cursor_change(shape)
    }
    fn request(&mut self, line1: &str, line2: Option<&str>, line3: Option<&str>, button: &str) {
        self.host.request(line1, line2, line3, button)
    }
    fn get_image_choice(&mut self, file: &dyn ImageFile) -> i32 {
        self.host.get_image_choice(file)
    }
    fn alpha_on(&self) -> i16 {
        self.host.alpha_on()
    }
}

impl Env for RealEnv<'_> {
    fn open_file(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        self.lib.open_file(name, None, None)
    }
    fn create_file(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        self.lib.create_file(name, None)
    }
    fn new_cmap(&mut self, colours: i32, width: i32, height: i32) -> Box<dyn ImageMem> {
        Box::new(ImageCmap::new(colours, width, height))
    }
    fn new_abgr(&mut self, width: i32, height: i32) -> Box<dyn ImageMem> {
        Box::new(ImageAbgr::new(width, height))
    }
    fn mem_to_rgba(&mut self, mem: &dyn ImageMem) -> Result<PictureRgba, Fault> {
        Ok(match mem_to_rgba(mem)? {
            Ok(p) | Err(p) => p,
        })
    }
    fn mem_to_dgt(&mut self, mem: &dyn ImageMem) -> Result<DgPicture, Fault> {
        mem_to_dgt(mem)
    }
    fn rgba_to_mem(&mut self, pic: &PictureRgba) -> Result<Box<dyn ImageMem>, Fault> {
        Ok(Box::new(rgba_to_mem(pic)?))
    }
}

fn c_str(s: &[u8]) -> &[u8] {
    s.iter().position(|&b| b == 0).map_or(s, |n| &s[..n])
}

/// `(int)` of an x87 value with the rounding set to truncation.
fn trunc(v: F80) -> i32 {
    v.to_i32(Round::Zero)
}

/// [soft 0x085b3f44 SI_ImageFromDG_Pict]: an index image (class `SI_ImageCMAP`) of the
/// picture's size with one colour per palette entry and one index per pixel.
///
/// Each channel is scaled from the palette's depth to 8 bits with a factor
/// `(float)(255.0 / ((1 << depth) - 1))` (the shift count is taken modulo 32, as the
/// processor does), and a colour byte is `(int)(0.5 + byte * factor)` cut to 8 bits, all
/// in the x87 unit's extended precision. The index of a pixel is the picture's 16-bit
/// value, **sign-extended** (the picture is read first row first and not turned).
/// The original reads the palette before it looks whether the picture is there; a picture
/// without a palette (or without pixels where it has an area) is a fault here.
pub fn image_from_dg_pict(dg: &DgPicture) -> Result<ImageCmap, Fault> {
    let pal = dg.palette.as_ref().ok_or(Fault("a picture without a palette: the original reads through a null pointer"))?;
    let mut img = ImageCmap::new(pal.count as i32, dg.width as i32, dg.height as i32);
    fill_from_dg(dg, &mut img)?;
    Ok(img)
}

/// [`image_from_dg_pict`] over an [`Env`]'s constructor.
pub fn image_from_dg_pict_with(env: &mut dyn Env, dg: &DgPicture) -> Result<Box<dyn ImageMem>, Fault> {
    let pal = dg.palette.as_ref().ok_or(Fault("a picture without a palette: the original reads through a null pointer"))?;
    let mut img = env.new_cmap(pal.count as i32, dg.width as i32, dg.height as i32);
    fill_from_dg(dg, &mut *img)?;
    Ok(img)
}

fn fill_from_dg(dg: &DgPicture, img: &mut dyn ImageMem) -> Result<(), Fault> {
    let pal = dg.palette.as_ref().ok_or(Fault("a picture without a palette: the original reads through a null pointer"))?;
    let mut factor = [0f32; 4];
    for (f, &d) in factor.iter_mut().zip(pal.bits.iter()) {
        let n = 1i32.wrapping_shl(d as u32 & 31).wrapping_sub(1);
        *f = (F80::from(255.0f64) / F80::from(n)).to_f32();
    }
    let (w, h) = (dg.width as i32, dg.height as i32);
    for (i, e) in pal.entries.iter().enumerate().take(pal.count as usize) {
        let mut c = [0u8; 4];
        for k in 0..4 {
            let v = F80::from(0.5f64) + F80::from(e.color[k] as i32) * factor[k];
            c[k] = trunc(v) as u8;
        }
        img.set_color_index_value4(i as i32, c)?;
    }
    let mut k = 0usize;
    for y in 0..h {
        for x in 0..w {
            let data = dg.data.as_ref().ok_or(Fault("an index picture without pixels: the original reads through a null pointer"))?;
            let v = *data.get(k).ok_or(Fault("more pixels than the index picture holds"))?;
            img.set_cpixel(x, y, v as i16 as i32)?;
            k += 1;
        }
    }
    Ok(())
}

/// [soft 0x085b41d0 SI_imageCISave]: saves an index picture to `name` as a colour-reduced
/// image. Answers `false` (nothing written, no message) for the types `pal` and `gif`, a
/// missing or empty name or type and a missing picture; a file that cannot be created
/// shows "Unable to save the image..." with an Ok button and answers `false`; otherwise
/// `true`, whether or not the format's writers succeed (their answers are ignored).
///
/// The photometric: type `dgt` asks for an index image (5) and ignores the answer; for any
/// other type a palette of up to 256 entries asks for it too and stays if the format can,
/// otherwise colour (4 samples when `si_alphaOn` is set and the format takes colour with
/// alpha (2), else colour (1) with 3 samples). Then width and height (unsigned 16-bit
/// from the picture), the description "Color Reduced", cursor 5, the header, the image,
/// close, cursor 0. The type text is read before anything else is tested: a null type is a
/// fault (the original compares through it).
pub fn image_ci_save(lib: &ImageLibrary, host: &mut dyn Glue, name: Option<&[u8]>, ty: Option<&[u8]>, dg: Option<&DgPicture>) -> Result<bool, Fault> {
    image_ci_save_with(&mut RealEnv { lib, host }, name, ty, dg)
}

/// [`image_ci_save`] over an [`Env`].
pub fn image_ci_save_with(env: &mut dyn Env, name: Option<&[u8]>, ty: Option<&[u8]>, dg: Option<&DgPicture>) -> Result<bool, Fault> {
    let ty = c_str(ty.ok_or(Fault("a null type: the original compares through it"))?);
    if ty == b"pal" || ty == b"gif" {
        return Ok(false);
    }
    let (Some(name), Some(dg)) = (name, dg) else { return Ok(false) };
    if c_str(name).is_empty() || ty.is_empty() {
        return Ok(false);
    }
    let Some(mut file) = env.create_file(name)? else {
        env.request("Unable to save the image...", None, None, "Ok");
        return Ok(false);
    };
    let img = image_from_dg_pict_with(env, dg)?;
    if ty == b"dgt" {
        file.set_photometric(5);
    } else {
        let count = dg.palette.as_ref().ok_or(Fault("a picture without a palette: the original reads through a null pointer"))?.count;
        if count > 0x100 || file.set_photometric(5) == 0 {
            if env.alpha_on() != 0 && file.set_photometric(2) != 0 {
                file.set_samples_per_pixel(4);
            } else {
                file.set_photometric(1);
                file.set_samples_per_pixel(3);
            }
        }
    }
    file.set_width(dg.width as i32);
    file.set_height(dg.height as i32);
    file.set_description(Some(b"Color Reduced"));
    env.cursor_change(5);
    file.write_header();
    file.write_image(&*img)?;
    file.close();
    env.cursor_change(0);
    Ok(true)
}

/// [soft 0x085b43e4 SI_imageRGBASave]: saves an RGBA picture to `name`. The types `pal` and
/// `gif` answer `true` at once, writing nothing; a missing or empty name or type, a missing
/// picture, or a file that cannot be created answer `false` without a message. Width and
/// height come from the image made of the picture, the description is "From Paint Tool.",
/// the format is asked for colour with alpha (2, then 4 samples) and else colour (1, 3
/// samples); with neither it shows "Error: RGB mode is not supported by this file format..."
/// with an Ok button and answers `false`. Otherwise cursor 5, header, image, close, cursor 0
/// and `true` (the writers' answers are ignored).
pub fn image_rgba_save(lib: &ImageLibrary, host: &mut dyn Glue, name: Option<&[u8]>, ty: Option<&[u8]>, pic: Option<&PictureRgba>) -> Result<bool, Fault> {
    image_rgba_save_with(&mut RealEnv { lib, host }, name, ty, pic)
}

/// [`image_rgba_save`] over an [`Env`].
pub fn image_rgba_save_with(env: &mut dyn Env, name: Option<&[u8]>, ty: Option<&[u8]>, pic: Option<&PictureRgba>) -> Result<bool, Fault> {
    let ty = c_str(ty.ok_or(Fault("a null type: the original compares through it"))?);
    if ty == b"pal" || ty == b"gif" {
        return Ok(true);
    }
    let (Some(name), Some(pic)) = (name, pic) else { return Ok(false) };
    if c_str(name).is_empty() || ty.is_empty() {
        return Ok(false);
    }
    let img = env.rgba_to_mem(pic)?;
    let Some(mut file) = env.create_file(name)? else { return Ok(false) };
    file.set_width(img.width());
    file.set_height(img.height());
    file.set_description(Some(b"From Paint Tool."));
    let ok = if file.set_photometric(2) != 0 {
        file.set_samples_per_pixel(4);
        true
    } else if file.set_photometric(1) != 0 {
        file.set_samples_per_pixel(3);
        true
    } else {
        false
    };
    if ok {
        env.cursor_change(5);
        file.write_header();
        file.write_image(&*img)?;
        file.close();
        env.cursor_change(0);
    } else {
        env.request("Error: RGB mode is not supported by this file format...", None, None, "Ok");
    }
    Ok(ok)
}

/// What `SI_imageLoad` leaves: the RGBA picture it answers and the index picture it stores
/// through its fourth argument.
#[derive(Debug, Default)]
pub struct Loaded {
    /// The answer: `None` for a missing type or name, a file that does not open or whose
    /// header does not read. The picture exists (with no pixels) even when the image's area
    /// is 0.
    pub rgba: Option<PictureRgba>,
    /// The index picture (rows turned over), only for an index file loaded as such.
    pub dg: Option<DgPicture>,
}

/// [soft 0x085b3590 SI_imageLoad]: loads the picture file `name`. Nothing happens (no cursor
/// change) for a missing type (the type is otherwise not used) or an empty name, or a file
/// that does not open. Otherwise cursor 5; a file of several images asks `get_image_choice`
/// and selects that one; a header that does not read ends with the file closed, cursor 0
/// and nothing.
///
/// An index file (photometric 5) with `as_index` not 0 is read into an index image of
/// `index_max_value + 1` colours, the file closed, the colour map copied entry by entry
/// (an entry the file does not give keeps the last one), and the image turned into an
/// index picture and an RGBA picture. When the index picture has no pixels it is thrown
/// away; otherwise its rows are **turned over** in place (row y with row h-1-y; the
/// middle row of an odd height stays) before it is stored. Every other file is read into
/// an `SI_ImageABGR` of its size, closed, and turned into the RGBA picture.
/// Then both images and the file are deleted and the cursor set to 0.
pub fn image_load(lib: &ImageLibrary, host: &mut dyn Glue, name: &[u8], ty: Option<&[u8]>, as_index: i16) -> Result<Loaded, Fault> {
    image_load_with(&mut RealEnv { lib, host }, name, ty, as_index)
}

/// [`image_load`] over an [`Env`].
pub fn image_load_with(env: &mut dyn Env, name: &[u8], ty: Option<&[u8]>, as_index: i16) -> Result<Loaded, Fault> {
    let mut out = Loaded::default();
    if ty.is_none() || c_str(name).is_empty() {
        return Ok(out);
    }
    let Some(mut file) = env.open_file(name)? else { return Ok(out) };
    env.cursor_change(5);
    if file.number_of_images() > 1 {
        let n = env.get_image_choice(&*file);
        file.set_current_image(n);
    }
    if !file.read_header() {
        file.close();
        env.cursor_change(0);
        return Ok(out);
    }
    if file.photometric() == 5 && as_index != 0 {
        let mut mem = env.new_cmap(file.index_max_value().wrapping_add(1), file.width(), file.height());
        file.read_image(&mut *mem)?;
        let mut last: Option<[u8; 3]> = None;
        let mut i = 0;
        while i <= file.index_max_value() {
            if let Some(v) = file.get_color_index_value(i) {
                last = Some(v);
            }
            let v = last.ok_or(Fault("a colour getter that writes nothing: the original reads stack bytes"))?;
            mem.set_color_index_value3(i, v)?;
            i += 1;
        }
        file.close();
        let mut dg = env.mem_to_dgt(&*mem)?;
        out.rgba = Some(env.mem_to_rgba(&*mem)?);
        if dg.width as u32 * dg.height as u32 != 0 {
            let (w, h) = (dg.width as usize, dg.height as usize);
            flip_rows(dg.data.as_mut().ok_or(Fault("an index picture without pixels"))?, w, h)?;
            out.dg = Some(dg);
        } else {
            env.dispose_dg();
        }
    } else {
        let mut mem = env.new_abgr(file.width(), file.height());
        file.read_image(&mut *mem)?;
        file.close();
        out.rgba = Some(env.mem_to_rgba(&*mem)?);
    }
    env.cursor_change(0);
    Ok(out)
}

/// The turning over of an index picture's rows as `SI_imageLoad` does it: a running index
/// `k` from the start swaps with the row from the end, row `h-1`, `h-2`, … while the row is
/// at least `(h+1)/2`.
fn flip_rows(data: &mut [u16], w: usize, h: usize) -> Result<(), Fault> {
    let mut k = 0usize;
    let mut y = h as i64 - 1;
    while y >= ((h as i64 + 1) >> 1) {
        for x in 0..w {
            let a = w * y as usize + x;
            if a >= data.len() || k >= data.len() {
                return Err(Fault("a row outside the index picture"));
            }
            data.swap(a, k);
            k += 1;
        }
        y -= 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dgt::DgPicture;
    use crate::image::ImageBase;

    fn dg(w: i32, h: i32, bits: [i32; 4]) -> DgPicture {
        DgPicture::allocate(w, h, 4, &bits)
    }

    #[test]
    fn from_dg_scales_colours_and_signs_indices() {
        let mut d = dg(2, 2, [5, 6, 5, 8]);
        d.palette.as_mut().unwrap().set_entry(1, [31, 63, 0, 255]).unwrap();
        let data = d.data.as_mut().unwrap();
        data[0] = 1;
        data[3] = 0xfffe;
        let img = image_from_dg_pict(&d).unwrap();
        assert_eq!(img.get_color_index_value4(1).unwrap(), Some([255, 255, 0, 255]));
        assert_eq!(img.get_cpixel(0, 0).unwrap(), 1);
        assert_eq!(img.get_cpixel(1, 1).unwrap() as i16, -2);
        assert_eq!(img.width(), 2);
    }

    #[test]
    fn flip_turns_rows_and_keeps_the_middle() {
        let mut d = vec![1u16, 2, 3, 4, 5, 6];
        flip_rows(&mut d, 2, 3).unwrap();
        assert_eq!(d, vec![5, 6, 3, 4, 1, 2]);
        let mut e = vec![1u16, 2, 3, 4];
        flip_rows(&mut e, 1, 4).unwrap();
        assert_eq!(e, vec![4, 3, 2, 1]);
    }

    #[test]
    fn factor_uses_the_shift_modulo_32() {
        // a depth of 32 gives a shift of 0, a divisor of 0: infinity, converted to nothing
        // sensible, but the same way the processor does it; just do not panic
        let d = dg(1, 1, [32, 8, 8, 8]);
        let _ = image_from_dg_pict(&d);
    }

    struct Host {
        log: Vec<String>,
    }
    impl Glue for Host {
        fn cursor_change(&mut self, s: i32) {
            self.log.push(format!("cursor {s}"));
        }
        fn request(&mut self, a: &str, _b: Option<&str>, _c: Option<&str>, d: &str) {
            self.log.push(format!("request {a} / {d}"));
        }
        fn get_image_choice(&mut self, _f: &dyn ImageFile) -> i32 {
            0
        }
        fn alpha_on(&self) -> i16 {
            0
        }
    }

    #[test]
    fn savers_refuse_pal_and_gif_without_a_library() {
        let lib = ImageLibrary::default();
        let mut h = Host { log: vec![] };
        let pic = PictureRgba { width: 1, height: 1, ratio: 1.0, fields: 0, pad: 1, data: vec![0; 4] };
        assert!(image_rgba_save(&lib, &mut h, Some(b"a.pal"), Some(b"pal"), Some(&pic)).unwrap());
        assert!(!image_rgba_save(&lib, &mut h, Some(b"a.tga"), Some(b"tga"), Some(&pic)).unwrap());
        assert!(!image_ci_save(&lib, &mut h, Some(b"a.gif"), Some(b"gif"), None).unwrap());
        let d = dg(1, 1, [8, 8, 8, 8]);
        assert!(!image_ci_save(&lib, &mut h, Some(b"a.xyz"), Some(b"xyz"), Some(&d)).unwrap());
        assert_eq!(h.log, vec!["request Unable to save the image... / Ok".to_string()]);
        let l = image_load(&lib, &mut h, b"nothing.tga", Some(b"tga"), 0).unwrap();
        assert!(l.rgba.is_none() && l.dg.is_none());
    }
}
