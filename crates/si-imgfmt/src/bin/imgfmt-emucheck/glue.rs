// SPDX-License-Identifier: MIT
//! Checks for the four glue routines of `quant.c` (`si_imgfmt::glue`) against the vectors of
//! `re/tools/img1/gen_glue.py`. The original runs with the image classes' constructors, the open
//! and create routines, the conversions, the cursor and the request box replaced by recorded
//! stand-ins, and with objects whose method tables are stand-ins too (the real methods lie above
//! the harness's licence cut-off). The port runs over an [`Env`] that replays the same tape: each
//! call the port makes (name, arguments, text) is held against the next recorded call, and
//! the answers come from the case's `ctl` buffer, which the stand-ins read as well.
//!
//! `EMUCHECK_MUTATE=1` flips one bit of the port's result; `EMUCHECK_MUTATE=calls` one bit of the
//! first compared argument of the first call.

use crate::dgt::dg_in;
use crate::util::mutate;
use si_imgfmt::dgt::DgPicture;
use si_imgfmt::glue::{self, Env, Glue};
use si_imgfmt::image::{Buffer, Channels, Image, ImageBase, ImageFile, ImageFileFields, ImageMem, Stream};
use si_imgfmt::tiff::Fault;
use si_picture::PictureRgba;
use si_x87::vectors::{Call, Case, Check, Verdict};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn calls_mutation() -> bool {
    std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "calls")
}

pub(crate) struct Tape {
    pub(crate) calls: Vec<Call>,
    pub(crate) idx: usize,
    pub(crate) bad: Option<String>,
    pub(crate) flipped: bool,
}

pub(crate) type Shared = Rc<RefCell<Tape>>;

impl Tape {
    pub(crate) fn fail(&mut self, why: String) {
        if self.bad.is_none() {
            self.bad = Some(why);
        }
    }

    /// The next call, which must be `name` with these arguments (`None`: not compared); the
    /// `pre:` entries in front of it come back too.
    pub(crate) fn take(&mut self, name: &str, args: &[Option<u32>]) -> Option<(Vec<Call>, Call)> {
        if self.bad.is_some() {
            return None;
        }
        let mut pre = Vec::new();
        while self.calls.get(self.idx).is_some_and(|k| k.name.starts_with("pre:")) {
            pre.push(self.calls[self.idx].clone());
            self.idx += 1;
        }
        let Some(k) = self.calls.get(self.idx).cloned() else {
            self.fail(format!("the port called {name} after the original's last call"));
            return None;
        };
        self.idx += 1;
        if k.name != name {
            self.fail(format!("the original called {}, the port {name}", k.name));
            return None;
        }
        for (i, a) in args.iter().enumerate() {
            let (Some(a), Some(theirs)) = (a, k.args.get(i).and_then(|w| w.value())) else { continue };
            let mut a = *a;
            if calls_mutation() && !self.flipped {
                self.flipped = true;
                a ^= 1;
            }
            if theirs as u32 != a {
                self.fail(format!("{name} argument {i}: the original {:#x}, the port {a:#x}", theirs as u32));
                return None;
            }
        }
        Some((pre, k))
    }

    pub(crate) fn take_text(&mut self, name: &str, args: &[Option<u32>], text: &[u8]) -> Option<Call> {
        let (pre, k) = self.take(name, args)?;
        let mut theirs: Option<Vec<u8>> = pre.iter().find(|p| p.name == "pre:text").map(|p| text_of(p));
        if calls_mutation() && !self.flipped {
            self.flipped = true;
            theirs = theirs.map(|mut t| {
                t.push(b'!');
                t
            });
        }
        if theirs.as_deref() != Some(text) {
            self.fail(format!("{name} text: the original {:?}, the port {:?}", theirs.map(|t| String::from_utf8_lossy(&t).into_owned()), String::from_utf8_lossy(text)));
            return None;
        }
        Some(k)
    }

    pub(crate) fn result(k: &Option<Call>) -> u32 {
        k.as_ref().and_then(|k| k.result.value()).unwrap_or(0) as u32
    }
}

pub(crate) fn text_of(pre: &Call) -> Vec<u8> {
    let mut v: Vec<u8> = pre.args.iter().map(|w| w.value().unwrap_or(0) as u8).collect();
    if v.last() == Some(&0) {
        v.pop();
    }
    v
}

pub(crate) fn c_str(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

/// The case's `ctl` words.
#[derive(Clone)]
struct Ctl(Vec<u32>);

impl Ctl {
    fn of(c: &Case) -> Ctl {
        Ctl(c.input("ctl").words().iter().map(|w| w.value().unwrap_or(0) as u32).collect())
    }
    fn colour(&self, i: i32) -> Option<[u8; 3]> {
        let ok = i >= 0 && i < 16 && (self.0[9] >> i) & 1 != 0;
        ok.then(|| {
            let b = 16 + 3 * i as usize;
            [self.0[b] as u8, self.0[b + 1] as u8, self.0[b + 2] as u8]
        })
    }
}

fn some(v: i32) -> Option<u32> {
    Some(v as u32)
}

// ---------------------------------------------------------------- the replayed objects

struct ReplayMem {
    tape: Shared,
    image: Image,
}

impl ImageBase for ReplayMem {
    fn image(&self) -> &Image {
        &self.image
    }
    fn image_mut(&mut self) -> &mut Image {
        &mut self.image
    }
}

fn unused<T>() -> Result<T, Fault> {
    Err(Fault("a method the glue does not call"))
}

impl ImageMem for ReplayMem {
    fn buffer(&self) -> Buffer<'_> {
        Buffer::None
    }
    fn set_size(&mut self, _w: i32, _h: i32) {}
    fn get_pixel(&self, _x: i32, _y: i32) -> Result<[u8; 4], Fault> {
        unused()
    }
    fn set_pixel(&mut self, _x: i32, _y: i32, _v: [u8; 4]) -> Result<(), Fault> {
        unused()
    }
    fn get_pixel16(&self, _x: i32, _y: i32) -> Result<[u16; 4], Fault> {
        unused()
    }
    fn set_pixel16(&mut self, _x: i32, _y: i32, _v: [u16; 4]) -> Result<(), Fault> {
        unused()
    }
    fn get_pixel_f(&self, _x: i32, _y: i32) -> Result<[f32; 4], Fault> {
        unused()
    }
    fn set_pixel_f(&mut self, _x: i32, _y: i32, _v: [f32; 4]) -> Result<(), Fault> {
        unused()
    }
    fn get_pixel_as(&self, _x: i32, _y: i32, _kind: i32) -> Result<Option<Channels>, Fault> {
        unused()
    }
    fn set_pixel_as(&mut self, _x: i32, _y: i32, _v: &Channels, _kind: i32) -> Result<(), Fault> {
        unused()
    }
    fn set_cpixel(&mut self, x: i32, y: i32, v: i32) -> Result<(), Fault> {
        self.tape.borrow_mut().take("mem.setCPixel", &[None, some(x), some(y), some(v)]);
        Ok(())
    }
    fn set_color_index_value3(&mut self, i: i32, v: [u8; 3]) -> Result<(), Fault> {
        self.tape.borrow_mut().take("mem.setColorIndexValue3", &[None, some(i), Some(v[0] as u32), Some(v[1] as u32), Some(v[2] as u32)]);
        Ok(())
    }
    fn set_color_index_value4(&mut self, i: i32, v: [u8; 4]) -> Result<(), Fault> {
        self.tape.borrow_mut().take("mem.setColorIndexValue4", &[None, some(i), Some(v[0] as u32), Some(v[1] as u32), Some(v[2] as u32), Some(v[3] as u32)]);
        Ok(())
    }
}

struct ReplayFile {
    tape: Shared,
    ctl: Ctl,
    f: ImageFileFields,
}

impl ImageBase for ReplayFile {
    fn image(&self) -> &Image {
        &self.f.image
    }
    fn image_mut(&mut self) -> &mut Image {
        &mut self.f.image
    }
    fn set_width(&mut self, v: i32) -> i32 {
        self.tape.borrow_mut().take("file.setWidth", &[None, some(v)]);
        1
    }
    fn set_height(&mut self, v: i32) -> i32 {
        self.tape.borrow_mut().take("file.setHeight", &[None, some(v)]);
        1
    }
    fn set_samples_per_pixel(&mut self, v: i32) -> i32 {
        self.tape.borrow_mut().take("file.setSamplesPerPixel", &[None, some(v)]);
        1
    }
    fn set_photometric(&mut self, v: i32) -> i32 {
        let k = self.tape.borrow_mut().take("file.setPhotometric", &[None, some(v)]).map(|x| x.1);
        Tape::result(&k) as i32
    }
}

impl ImageFile for ReplayFile {
    fn fields(&self) -> &ImageFileFields {
        &self.f
    }
    fn fields_mut(&mut self) -> &mut ImageFileFields {
        &mut self.f
    }
    fn set_current_image(&mut self, v: i32) -> bool {
        self.tape.borrow_mut().take("file.setCurrentImage", &[None, some(v)]);
        true
    }
    fn get_color_index_value(&self, i: i32) -> Option<[u8; 3]> {
        self.tape.borrow_mut().take("file.getColorIndexValue", &[None, some(i)]);
        self.ctl.colour(i)
    }
    fn index_max_value(&self) -> i32 {
        self.tape.borrow_mut().take("file.indexMaxValue", &[None]);
        self.ctl.0[5] as i32
    }
    fn open_stream(&mut self, _s: Stream, _m: &[u8]) -> bool {
        false
    }
    fn open_path(&mut self, _n: &[u8], _m: &[u8]) -> bool {
        false
    }
    fn close(&mut self) -> bool {
        self.tape.borrow_mut().take("file.close", &[None]);
        true
    }
    fn read_header(&mut self) -> bool {
        let k = self.tape.borrow_mut().take("file.readHeader", &[None]).map(|x| x.1);
        Tape::result(&k) as u16 != 0
    }
    fn write_header(&mut self) -> bool {
        self.tape.borrow_mut().take("file.writeHeader", &[None]);
        true
    }
    fn read_next_line(&mut self, _m: &mut dyn ImageMem, _l: i32) -> Result<bool, Fault> {
        unused()
    }
    fn write_next_line(&mut self, _m: &dyn ImageMem, _l: i32) -> Result<bool, Fault> {
        unused()
    }
    fn read_image(&mut self, _mem: &mut dyn ImageMem) -> Result<bool, Fault> {
        self.tape.borrow_mut().take("file.readImage", &[None, None]);
        Ok(true)
    }
    fn write_image(&mut self, _mem: &dyn ImageMem) -> Result<bool, Fault> {
        self.tape.borrow_mut().take("file.writeImage", &[None, None]);
        Ok(true)
    }
    fn set_description(&mut self, d: Option<&[u8]>) {
        self.tape.borrow_mut().take_text("setDescription", &[None], d.map(c_str).unwrap_or(b""));
    }
}

/// The environment of a case: the tape, the case's answers.
struct Replay {
    tape: Shared,
    ctl: Ctl,
    alpha: i16,
    dgdat: Vec<u16>,
}

impl Replay {
    fn new(c: &Case) -> Replay {
        let tape = Rc::new(RefCell::new(Tape { calls: c.calls.clone(), idx: 0, bad: None, flipped: false }));
        let alpha = c.try_input("alpha").and_then(|f| f.words().first().and_then(|w| w.value())).unwrap_or(0) as u16 as i16;
        let dgdat = c.try_input("dgdat").map(|f| f.words().iter().map(|w| w.value().unwrap_or(0) as u16).collect()).unwrap_or_default();
        Replay { tape, ctl: Ctl::of(c), alpha, dgdat }
    }

    fn mem(&self, w: i32, h: i32) -> Box<dyn ImageMem> {
        let mut image = Image::new();
        image.width = w;
        image.height = h;
        Box::new(ReplayMem { tape: self.tape.clone(), image })
    }

    fn file(&self) -> Box<dyn ImageFile> {
        let mut f = ImageFileFields::new();
        f.image.width = self.ctl.0[1] as i32;
        f.image.height = self.ctl.0[2] as i32;
        f.image.photometric = self.ctl.0[3] as i32;
        f.number_of_images = self.ctl.0[4] as i32;
        Box::new(ReplayFile { tape: self.tape.clone(), ctl: self.ctl.clone(), f })
    }

    fn open_or_create(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        let k = self.tape.borrow_mut().take_text("open_create", &[None], c_str(name));
        Ok((Tape::result(&k) != 0).then(|| self.file()))
    }
}

impl Glue for Replay {
    fn cursor_change(&mut self, shape: i32) {
        self.tape.borrow_mut().take("SI_cursorChange", &[some(shape)]);
    }
    fn request(&mut self, l1: &str, l2: Option<&str>, l3: Option<&str>, button: &str) {
        let mut t = self.tape.borrow_mut();
        let Some((pre, _)) = t.take("SI_dialogRequest", &[]) else { return };
        let mut parts: Vec<u32> = Vec::new();
        for s in [Some(l1), l2, l3, Some(button)] {
            match s {
                None => parts.push(0xFE),
                Some(s) => {
                    parts.extend(s.bytes().map(|b| b as u32));
                    parts.push(0xFF);
                }
            }
        }
        if calls_mutation() && !t.flipped {
            t.flipped = true;
            parts[0] ^= 1;
        }
        let theirs: Option<Vec<u32>> = pre.iter().find(|p| p.name == "pre:request").map(|p| p.args.iter().map(|w| w.value().unwrap_or(0) as u32).collect());
        if theirs.as_ref() != Some(&parts) {
            t.fail(format!("the request box: the original {theirs:?}, the port {parts:?}"));
        }
    }
    fn get_image_choice(&mut self, _file: &dyn ImageFile) -> i32 {
        let k = self.tape.borrow_mut().take("SI_getImageChoice", &[None]).map(|x| x.1);
        Tape::result(&Some(k).flatten()) as i32
    }
    fn alpha_on(&self) -> i16 {
        self.alpha
    }
}

impl Env for Replay {
    fn open_file(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        self.open_or_create(name)
    }
    fn create_file(&mut self, name: &[u8]) -> Result<Option<Box<dyn ImageFile>>, Fault> {
        self.open_or_create(name)
    }
    fn new_cmap(&mut self, colours: i32, width: i32, height: i32) -> Box<dyn ImageMem> {
        self.tape.borrow_mut().take("SI_ImageCMAP", &[None, some(colours), some(width), some(height)]);
        self.mem(width, height)
    }
    fn new_abgr(&mut self, width: i32, height: i32) -> Box<dyn ImageMem> {
        self.tape.borrow_mut().take("SI_ImageABGR", &[None, some(width), some(height)]);
        self.mem(width, height)
    }
    fn mem_to_rgba(&mut self, _mem: &dyn ImageMem) -> Result<PictureRgba, Fault> {
        self.tape.borrow_mut().take("SI_ImageMem2RGBA", &[None, None]);
        let w = &self.ctl.0;
        Ok(PictureRgba { width: w[12] as u16 as i16, height: (w[12] >> 16) as u16 as i16, ratio: f32::from_bits(w[13]), fields: w[14] as u16 as i16, pad: (w[14] >> 16) as u16 as i16, data: Vec::new() })
    }
    fn mem_to_dgt(&mut self, _mem: &dyn ImageMem) -> Result<DgPicture, Fault> {
        self.tape.borrow_mut().take("imageMem2DGT", &[None, None]);
        let (w, h) = (self.ctl.0[10] as u16, self.ctl.0[11] as u16);
        let n = w as usize * h as usize;
        let mut d = DgPicture { width: w, height: h, size: 0, palette: None, data: None, bytes: None, rect: [0; 4] };
        if n > 0 {
            d.data = Some(self.dgdat.iter().copied().chain(std::iter::repeat(0)).take(n).collect());
        }
        Ok(d)
    }
    fn rgba_to_mem(&mut self, pic: &PictureRgba) -> Result<Box<dyn ImageMem>, Fault> {
        self.tape.borrow_mut().take("SI_PictRGBA2ImageMem", &[None]);
        Ok(self.mem(pic.width as i32, pic.height as i32))
    }
    fn dispose_dg(&mut self) {
        self.tape.borrow_mut().take("SI_dgPictureDispose", &[None]);
    }
}

// ---------------------------------------------------------------- the checks

fn word32(c: &Case, name: &str) -> u32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32
}

fn is_ref(w: &si_x87::vectors::Word) -> bool {
    matches!(w, si_x87::vectors::Word::Ref { .. })
}

fn string_arg(c: &Case, p: &str, buf: &str) -> Option<Vec<u8>> {
    is_ref(&c.input(p).words()[0]).then(|| c_str(&c.input(buf).to_bytes()).to_vec())
}

/// The tape all made and nothing left over.
fn finished(r: &Replay) -> Verdict {
    let t = r.tape.borrow();
    match &t.bad {
        Some(why) => Verdict::Different(why.clone()),
        None if t.idx != t.calls.len() => Verdict::Different(format!("{} recorded calls were not made by the port", t.calls.len() - t.idx)),
        None => Verdict::Same,
    }
}

fn flip(v: u32) -> u32 {
    if mutate() {
        v ^ 1
    } else {
        v
    }
}

fn same_value(what: &str, theirs: Option<u128>, ours: u32, mask: u32) -> Verdict {
    match theirs {
        Some(t) if (t as u32 & mask) == (flip(ours) & mask) => Verdict::Same,
        Some(t) => Verdict::Different(format!("{what}: the original {:#x}, the port {:#x}", t as u32 & mask, ours & mask)),
        None => Verdict::Different(format!("{what}: not written in the original")),
    }
}

fn out_value(c: &Case, name: &str, i: usize) -> Option<u128> {
    c.try_output(name).and_then(|f| f.words().get(i).and_then(|w| w.value()))
}

/// Whether the original's pointer result is not null.
fn ptr_nonnull(c: &Case) -> bool {
    match c.try_output("ptr").map(|f| &f.words()[0]) {
        Some(w) if is_ref(w) => true,
        Some(w) => w.value().unwrap_or(0) != 0,
        None => false,
    }
}

fn refused(c: &Case, f: Fault) -> Verdict {
    crate::pict::refused(c, f.0)
}

fn from_dg(c: &Case) -> Verdict {
    if !is_ref(&c.input("p_dg").words()[0]) {
        return crate::pict::refused(c, "a null picture: the original reads through it");
    }
    let dg = dg_in(c);
    let mut r = Replay::new(c);
    if dg.palette.is_none() {
        return crate::pict::refused(c, "no palette");
    }
    let got = glue::image_from_dg_pict_with(&mut r, &dg);
    match got {
        Err(f) => refused(c, f),
        Ok(_) => finished(&r).and(|| Verdict::from(ptr_nonnull(c) != mutate(), || "the original returned null".into())),
    }
}

fn type_text(c: &Case) -> Option<Vec<u8>> {
    string_arg(c, "p_type", "type")
}

fn ci_save(c: &Case) -> Verdict {
    let name = string_arg(c, "p_name", "name");
    let Some(ty) = type_text(c) else { return crate::pict::refused(c, "a null type: the original compares through it") };
    let dg = is_ref(&c.input("p_dg").words()[0]).then(|| dg_in(c));
    let mut r = Replay::new(c);
    let got = glue::image_ci_save_with(&mut r, name.as_deref(), Some(&ty), dg.as_ref());
    match got {
        Err(f) => refused(c, f),
        Ok(ok) => finished(&r).and(|| same_value("result", out_value(c, "ax", 0), ok as u32, 0xffff)),
    }
}

fn rgba_save(c: &Case) -> Verdict {
    let name = string_arg(c, "p_name", "name");
    let Some(ty) = type_text(c) else { return crate::pict::refused(c, "a null type: the original compares through it") };
    let pic = is_ref(&c.input("p_pic").words()[0]).then(|| {
        let w = c.input("pic").words();
        let g = |i: usize| w[i].value().unwrap_or(0);
        PictureRgba { width: g(0) as u16 as i16, height: g(1) as u16 as i16, ratio: f32::from_bits(g(2) as u32), fields: g(3) as u16 as i16, pad: g(4) as u16 as i16, data: Vec::new() }
    });
    let mut r = Replay::new(c);
    let got = glue::image_rgba_save_with(&mut r, name.as_deref(), Some(&ty), pic.as_ref());
    match got {
        Err(f) => refused(c, f),
        Ok(ok) => finished(&r).and(|| same_value("result", out_value(c, "ax", 0), ok as u32, 0xffff)),
    }
}

fn load(c: &Case) -> Verdict {
    let name = string_arg(c, "p_name", "name").unwrap_or_default();
    let ty = type_text(c);
    let idx = word32(c, "v_idx") as u16 as i16;
    let mut r = Replay::new(c);
    let got = match glue::image_load_with(&mut r, &name, ty.as_deref(), idx) {
        Ok(l) => l,
        Err(f) => return refused(c, f),
    };
    let finish = finished(&r);
    let nz = out_value(c, "nz", 0).unwrap_or(0) as u32;
    finish
        .and(|| same_value("result is not null", Some(nz as u128), got.rgba.is_some() as u32, 1))
        .and(|| match (&got.rgba, c.try_output("rec")) {
            (Some(p), Some(rec)) => {
                let w = rec.words();
                let g = |i: usize| w.get(i).and_then(|x| x.value());
                same_value("picture size", g(0), (p.width as u16 as u32) | ((p.height as u16 as u32) << 16), !0)
                    .and(|| same_value("picture ratio", g(1), p.ratio.to_bits(), !0))
                    .and(|| same_value("picture fields", g(2), (p.fields as u16 as u32) | ((p.pad as u16 as u32) << 16), !0))
            }
            (None, None) => Verdict::Same,
            (a, b) => Verdict::Different(format!("record: the port {}, the original {}", a.is_some(), b.is_some())),
        })
        .and(|| same_value("index picture stored", out_value(c, "dgnz", 0), got.dg.is_some() as u32, 1))
        .and(|| match &got.dg {
            Some(d) => same_value("index picture width", out_value(c, "dgsize", 0), d.width as u32, 0xffff)
                .and(|| same_value("index picture height", out_value(c, "dgsize", 1), d.height as u32, 0xffff))
                .and(|| match (&d.data, c.try_output("dgdata")) {
                    (Some(data), Some(f)) => {
                        let theirs: Vec<u128> = f.words().iter().map(|w| w.value().unwrap_or(0)).collect();
                        let mut ours: Vec<u128> = data.iter().map(|&v| v as u128).collect();
                        if mutate() && !ours.is_empty() {
                            ours[0] ^= 1;
                        }
                        Verdict::from(theirs == ours, || format!("index data: the original {theirs:?}, the port {ours:?}"))
                    }
                    (None, None) => Verdict::Same,
                    _ => Verdict::Different("index data present on one side only".into()),
                }),
            None => Verdict::Same,
        })
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_ImageFromDG_Pict" => |c: &Case| from_dg(c),
        "SI_imageCISave" => |c: &Case| ci_save(c),
        "SI_imageRGBASave" => |c: &Case| rgba_save(c),
        "SI_imageLoad" => |c: &Case| load(c),
        _ => return None,
    })
}
