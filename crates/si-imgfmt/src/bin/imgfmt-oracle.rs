// SPDX-License-Identifier: MIT
//! The port against the outputs of the original's standalone converters (finding 0014, fifth
//! pass, "The real converters"; the outputs: finding 0031, second pass).
//!
//!   (ulimit -v 2000000; imgfmt-oracle re/out/oracle/tools [--keep <directory>])
//!
//! The converters (`soft2tiff`, `pict2soft`, …) link the same picture-library code as `soft`
//! (`re/tools/si-imgfmt/samecode.py`: every library function they define is the same
//! instruction for instruction, addresses aside); their `main`s read or write the program's
//! picture file and call one library routine. So the port's routine on the same input must give
//! the converter's file.
//!
//! - **To a format** (`soft2<format>`): the sample picture read by the port's picture reader,
//!   written by the port's routine with the arguments the converter's `main` passes; compared
//!   byte for byte with the converter's file. Bytes the port knows to be undefined (what the
//!   allocator or the stack left, the date, a file descriptor) are left out and counted. Where
//!   the oracle ran a command twice (`<variant>-repeat`), every byte that differs between the
//!   original's two runs must be one the port calls undefined; the count of those that are not
//!   is reported.
//! - **Back to the picture** (`<format>2soft`): the converter's input (the original's own
//!   foreign file) read by the port's reader and written by the port's picture writer with the
//!   comment the original's file carries (the `main` makes it from the file name); compared
//!   whole, without the header's padding word (bytes 102 and 103: stale stack in the original,
//!   finding 0003). Where the port's reader leaves pixels undefined the planes are compared
//!   instead, on the defined pixels.
//!
//! Prints one block a converter and variant; with `--keep` writes the port's differing files.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use si_imgfmt::{alias, aurora, everest, pict, raw, sgi, targa, tiff, vertigo, yuv};
use si_picture::Picture;

struct Row {
    id: String,
    input: String,
    args: Vec<String>,
    status: String,
    outputs: Vec<String>,
}

fn manifest(dir: &Path) -> Vec<Row> {
    let Ok(text) = fs::read_to_string(dir.join("manifest.tsv")) else { return Vec::new() };
    text.lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let outputs = f.get(5).map_or(Vec::new(), |o| {
                o.split(',').filter(|s| !s.is_empty()).map(|s| s.rsplit_once(':').map_or(s, |x| x.0).to_string()).collect()
            });
            Row {
                id: f[0].to_string(),
                input: f[1].to_string(),
                args: f.get(2).map_or(Vec::new(), |a| a.split(' ').map(str::to_string).collect()),
                status: f.get(3).unwrap_or(&"").to_string(),
                outputs,
            }
        })
        .collect()
}

/// What the port makes for one command.
enum Made {
    /// Files by the suffix after the command's prefix ("" for the one output); `None` bytes
    /// are undefined in the original (the port cannot say what they are).
    Files(Vec<(String, Vec<Option<u8>>)>),
    /// The routine refuses (a message, no file).
    Refused(String),
    /// The port stops where the original's behaviour is undefined.
    Fault(String),
}

fn defined(b: &[u8]) -> Vec<Option<u8>> {
    b.iter().map(|&x| Some(x)).collect()
}

fn one(b: &[u8]) -> Made {
    Made::Files(vec![(String::new(), defined(b))])
}

/// A byte buffer with the given ranges marked undefined.
fn masked(b: &[u8], ranges: &[std::ops::Range<usize>]) -> Vec<Option<u8>> {
    let mut v = defined(b);
    for r in ranges {
        for i in r.clone() {
            if let Some(x) = v.get_mut(i) {
                *x = None;
            }
        }
    }
    v
}

#[derive(Default)]
struct Tally {
    classes: BTreeMap<String, usize>,
    lines: String,
    /// Samples the original's round trip changes, of them the port's same and undefined.
    rt_changed: usize,
    rt_same: usize,
    rt_undef: usize,
}

impl Tally {
    fn add(&mut self, id: &str, class: &str, detail: &str) {
        *self.classes.entry(class.to_string()).or_default() += 1;
        let _ = writeln!(self.lines, "{id}\t{class}\t{detail}");
    }
    fn summary(&self) -> String {
        self.classes.iter().map(|(c, n)| format!("{c}: {n}")).collect::<Vec<_>>().join(", ")
    }
}

/// Port bytes against the original's: (differing defined bytes, first, undefined bytes).
fn compare(port: &[Option<u8>], orig: &[u8]) -> (usize, Option<usize>, usize) {
    let mut n = 0;
    let mut first = None;
    let mut undef = 0;
    for i in 0..port.len().max(orig.len()) {
        match (port.get(i), orig.get(i)) {
            (Some(None), Some(_)) => undef += 1,
            (Some(Some(a)), Some(b)) if a == b => {}
            _ => {
                n += 1;
                first.get_or_insert(i);
            }
        }
    }
    (n, first, undef)
}

/// The original's output files of one command: suffix after `prefix` -> bytes.
fn originals(dir: &Path, prefix: &str, row: &Row) -> Vec<(String, Vec<u8>)> {
    row.outputs
        .iter()
        .filter_map(|o| Some((o.strip_prefix(prefix)?.to_string(), fs::read(dir.join(o)).ok()?)))
        .collect()
}

struct Ctx {
    tools: PathBuf,
    keep: Option<PathBuf>,
    out: String,
}

/// Judges one command: the port's files against the original's, and (with `repeat`, the
/// original's files of the same command run again) whether the bytes that changed between
/// the original's runs are all undefined in the port.
fn judge(cx: &mut Ctx, t: &mut Tally, id: &str, made: Made, orig: &[(String, Vec<u8>)], repeat: Option<(&[(String, Vec<u8>)], Vec<Vec<Option<u8>>>)>, status: &str) {
    let crash = if status == "139" { " (original: segmentation fault after writing)" } else { "" };
    let files = match made {
        Made::Refused(e) if orig.is_empty() => return t.add(id, "both refuse", &e),
        Made::Refused(e) => return t.add(id, "port refuses, original wrote", &e),
        Made::Fault(e) if orig.is_empty() && status == "139" => return t.add(id, "both stop (port: undefined; original: segmentation fault)", &e),
        Made::Fault(e) if orig.is_empty() => return t.add(id, "port stops (undefined), original wrote nothing", &format!("{e}; exit status {status}")),
        Made::Fault(e) => return t.add(id, "port stops (undefined), original wrote", &format!("{e}; exit status {status}")),
        Made::Files(f) => f,
    };
    if orig.is_empty() {
        return t.add(id, "original wrote nothing, port did", &format!("exit status {status}"));
    }
    let mut detail = String::new();
    let (mut same, mut undef_total, mut unstable_outside) = (true, 0usize, 0usize);
    for (suffix, o) in orig {
        let p = files.iter().find(|(s, _)| s == suffix).or_else(|| (files.len() == 1 && files[0].0.is_empty() && orig.len() == 1).then(|| &files[0]));
        let Some((_, p)) = p else {
            same = false;
            let _ = write!(detail, "{suffix}: the port wrote no such file; ");
            continue;
        };
        let (n, first, undef) = compare(p, o);
        undef_total += undef;
        if n > 0 {
            same = false;
            let _ = write!(detail, "{suffix}: {n} bytes differ, first at {}, sizes port {} original {}; ", first.unwrap_or(0), p.len(), o.len());
            if let Some(dir) = &cx.keep {
                let _ = fs::create_dir_all(dir);
                let bytes: Vec<u8> = p.iter().map(|b| b.unwrap_or(0)).collect();
                let _ = fs::write(dir.join(format!("{id}{suffix}")), bytes);
            }
        }
        if let Some((r, rp)) = repeat.as_ref().and_then(|(r, rp)| {
            let k = r.iter().position(|(s, _)| s.ends_with(suffix.as_str()) || (orig.len() == 1 && r.len() == 1))?;
            Some((&r[k].1, rp.get(k).or(rp.first())?))
        }) {
            // bytes that change between the original's two runs although the port, given
            // each run's own inputs (output name, date), makes the same defined byte
            let changed: Vec<usize> = (0..o.len().max(r.len())).filter(|&i| o.get(i) != r.get(i)).collect();
            let outside = changed.iter().filter(|&&i| matches!((p.get(i), rp.get(i)), (Some(Some(a)), Some(Some(b))) if a == b)).count();
            unstable_outside += outside;
            if !changed.is_empty() {
                let _ = write!(detail, "{suffix}: {} bytes change between the original's runs, {outside} of them defined in the port; ", changed.len());
            }
        }
    }
    for (s, _) in &files {
        if !s.is_empty() && !orig.iter().any(|(o, _)| o == s) {
            same = false;
            let _ = write!(detail, "{s}: the original wrote no such file; ");
        }
    }
    let class = match (same, undef_total) {
        (true, 0) => "identical".to_string(),
        (true, _) => "identical outside the undefined bytes".to_string(),
        (false, _) => "differing".to_string(),
    };
    if undef_total > 0 {
        let _ = write!(detail, "undefined in the port: {undef_total} bytes; ");
    }
    if unstable_outside > 0 {
        let _ = write!(detail, "UNSTABLE BYTES DEFINED IN THE PORT: {unstable_outside}; ");
        *t.classes.entry("(runs of the original differ on bytes the port defines)".into()).or_default() += 1;
    }
    t.add(id, &(class + crash), detail.trim_end());
}

fn read_pic(bytes: &[u8]) -> Result<Picture, String> {
    Picture::read(bytes).map(|f| f.picture).map_err(|e| format!("picture reader: {e:?}"))
}

/// The comment field of a picture file, up to its first zero byte.
fn comment_of(pic_file: &[u8]) -> Vec<u8> {
    let f = &pic_file[8.min(pic_file.len())..88.min(pic_file.len())];
    f[..f.iter().position(|&b| b == 0).unwrap_or(f.len())].to_vec()
}

/// A picture file as the converters' `main`s write it, with the padding word undefined.
fn pic_file(p: &Picture, comment: &[u8]) -> Made {
    Made::Files(vec![(String::new(), masked(&p.write(Some(comment), false), &[102..104]))])
}

/// The 20-byte date block of a TIFF file the original wrote (`%Y:%m:%d %H:%M:%S` and a NUL):
/// the clock's answer, which the port takes as an argument.
fn tiff_date(orig: &[u8]) -> Vec<Option<u8>> {
    let is_d = |b: u8| b.is_ascii_digit();
    for i in 0..orig.len().saturating_sub(20) {
        let w = &orig[i..i + 20];
        if w[4] == b':' && w[7] == b':' && w[10] == b' ' && w[13] == b':' && w[16] == b':' && w[19] == 0 && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18].iter().all(|&k| is_d(w[k])) {
            return defined(w);
        }
    }
    vec![None; 20]
}

/// The output name a converter's `main` hands the library: the second argument.
fn out_name(row: &Row) -> Vec<u8> {
    row.args.get(1).map(|s| s.as_bytes().to_vec()).unwrap_or_default()
}

type From = fn(&Row, &[u8], &Picture, &[u8]) -> Made;

fn from_table() -> Vec<(&'static str, &'static str, &'static str, From)> {
    fn tga(p: &Picture, raw: bool) -> Made {
        match targa::write(p, raw) {
            Ok(b) => one(&b),
            Err(e) => Made::Refused(format!("{e:?}")),
        }
    }
    fn yuv_w(p: &Picture, key: bool) -> Made {
        match yuv::write_yuv(p, key, None) {
            Ok(Some(b)) => one(&b),
            Ok(None) => one(&[]),
            Err(e) => Made::Fault(format!("{e:?}")),
        }
    }
    fn aur(b: &[u8], a: u8) -> Made {
        match aurora::export_aurora(b, a) {
            Ok(x) => one(&x),
            Err(e) => Made::Refused(format!("{e:?}")),
        }
    }
    fn tif(row: &Row, p: &Picture, orig: &[u8], mac: i32, planar: i16, raw: i16) -> Made {
        let date = tiff_date(orig);
        let mut t = tiff::Tiff::default();
        match t.write(p, &out_name(row), mac, planar, raw, &date) {
            Ok(w) => Made::Files(vec![(String::new(), w.file)]),
            Err(e) => Made::Fault(e.0.to_string()),
        }
    }
    fn vtg(p: &Picture) -> Made {
        match vertigo::write_vertigo(p) {
            Ok(b) => Made::Files(vec![(String::new(), b)]),
            Err(e) => Made::Fault(e.0.to_string()),
        }
    }
    fn pct(row: &Row, p: &Picture, mac: i32) -> Made {
        match pict::write_pict(p, &out_name(row), mac) {
            Ok(b) => one(&b),
            Err(e) => Made::Fault(e.0.to_string()),
        }
    }
    vec![
        ("soft2alias", "default", "SI_pictureExportAlias", |_, b, _, _| match alias::export_alias(b) {
            Ok(x) => one(&x),
            Err(e) => Made::Refused(format!("{e:?}")),
        }),
        ("soft2aurora", "default", "SI_pictureExportAurora (alpha 0xff)", |_, b, _, _| aur(b, 0xff)),
        ("soft2aurora", "a128", "SI_pictureExportAurora (alpha 128)", |_, b, _, _| aur(b, 128)),
        ("soft2tga", "default", "SI_pictureWriteTarga, run-length", |_, _, p, _| tga(p, false)),
        ("soft2tga", "R", "SI_pictureWriteTarga, raw", |_, _, p, _| tga(p, true)),
        ("soft2sgi", "default", "SI_pictureWriteSgi", |_, _, p, _| match sgi::picture_write_sgi(p) {
            Ok(sgi::Written::File(b)) | Ok(sgi::Written::HeaderOnly(b)) => Made::Files(vec![(String::new(), masked(&b, &sgi::HEADER_UNDEFINED))]),
            Err(e) => Made::Fault(format!("{e:?}")),
        }),
        ("soft2yuv", "default", "SI_pictureWriteYuv", |_, _, p, _| yuv_w(p, false)),
        ("soft2yuv", "b", "SI_pictureWriteYuv, with key", |_, _, p, _| yuv_w(p, true)),
        ("soft2raw", "default", "SI_pictureWriteRaw's rule (the main writes the planes itself)", |_, _, p, _| match raw::write_raw(p) {
            Ok(planes) => Made::Files(planes.into_iter().zip(raw::RAW_EXTENSIONS).filter_map(|(b, e)| b.map(|b| (e.to_string(), defined(&b)))).collect()),
            Err(e) => Made::Refused(format!("{e:?}")),
        }),
        ("soft2rgb", "default", "SI_pictureWriteRgb's rule (the main writes the pixels itself)", |_, _, p, _| {
            let a = p.planes[3].is_some();
            let ext = if a { raw::RGBA_EXTENSION } else { raw::RGB_EXTENSION };
            match raw::write_rgb(p, a) {
                Ok(b) => Made::Files(vec![(ext.to_string(), defined(&b))]),
                Err(e) => Made::Refused(format!("{e:?}")),
            }
        }),
        ("soft2rgb", "a", "SI_pictureWriteRgb's rule, no alpha", |_, _, p, _| match raw::write_rgb(p, false) {
            Ok(b) => Made::Files(vec![(raw::RGB_EXTENSION.to_string(), defined(&b))]),
            Err(e) => Made::Refused(format!("{e:?}")),
        }),
        ("soft2tiff", "default", "SI_pictureWriteTiff (planar 2, PackBits)", |r, _, p, o| tif(r, p, o, 0, 2, 0)),
        ("soft2tiff", "R", "SI_pictureWriteTiff (planar 2, raw)", |r, _, p, o| tif(r, p, o, 0, 2, 1)),
        ("soft2tiff", "p", "SI_pictureWriteTiff (planar 1)", |r, _, p, o| tif(r, p, o, 0, 1, 0)),
        ("soft2tiff", "m", "SI_pictureWriteTiff (MacBinary 0x80, planar 2, PackBits)", |r, _, p, o| tif(r, p, o, 0x80, 2, 0)),
        ("soft2everest", "default", "SI_pictureExportEverest", |_, b, _, _| match everest::export_everest(b) {
            Ok(Ok(x)) => Made::Files(vec![(String::new(), x)]),
            Ok(Err(e)) => Made::Refused(format!("{e:?}")),
            Err(e) => Made::Fault(e.0.to_string()),
        }),
        ("soft2vertigo", "default", "SI_pictureWriteVertigo", |_, _, p, _| vtg(p)),
        ("soft2vertigo", "p", "SI_pictureWriteVertigo (-p only names the file)", |_, _, p, _| vtg(p)),
        ("soft2pict", "default", "SI_pictureWritePict", |r, _, p, _| pct(r, p, 0)),
        ("soft2pict", "m", "SI_pictureWritePict (MacBinary 0x80)", |r, _, p, _| pct(r, p, 0x80)),
    ]
}

type Back = fn(&Row, &[u8], (i16, i16), &BTreeMap<String, Vec<u8>>, &[u8], &[u8]) -> Made;

/// Reading a TIFF or PICT file into planes that may hold undefined bytes, then the picture file.
fn planes_to_file(width: i16, height: i16, ratio: f32, fields: i16, planes: [Option<Vec<Option<u8>>>; 4], comment: &[u8], orig: &[u8]) -> Made {
    if planes.iter().flatten().all(|p| p.iter().all(Option::is_some)) {
        let planes = planes.map(|p| p.map(|v| v.into_iter().map(|b| b.unwrap()).collect()));
        return pic_file(&Picture { width, height, ratio, fields, pad: 0, planes }, comment);
    }
    // Some pixels undefined: compare the planes with the original's on the defined pixels;
    // reported through a one-byte-a-pixel stand-in (the planes one after another).
    let Ok(o) = read_pic(orig) else { return Made::Refused("the original's picture does not read".into()) };
    let mut port = Vec::new();
    let mut theirs = Vec::new();
    for c in 0..4 {
        match (&planes[c], &o.planes[c]) {
            (Some(p), Some(q)) => {
                port.extend_from_slice(p);
                theirs.extend_from_slice(q);
            }
            (None, None) => {}
            _ => {
                port.push(Some(1));
                theirs.push(0);
            }
        }
    }
    if (o.width, o.height, o.fields) != (width, height, fields) || o.ratio.to_bits() != ratio.to_bits() {
        port.push(Some(1));
        theirs.push(0);
    }
    Made::Files(vec![("#planes".into(), port), ("#orig".into(), defined(&theirs))])
}

fn back_table() -> Vec<(&'static str, &'static str, &'static str, Back)> {
    fn tga(b: &[u8], c: &[u8]) -> Made {
        match targa::read(b, false, false, 1.0) {
            Ok(r) => pic_file(&r.picture, c),
            Err(e) => Made::Refused(format!("{e:?}")),
        }
    }
    fn tif(row: &Row, b: &[u8], c: &[u8], mac: i32, orig: &[u8]) -> Made {
        let mut t = tiff::Tiff::default();
        match t.read(Some(b), mac, row.input.as_bytes(), None, None) {
            Err(e) => Made::Fault(e.0.to_string()),
            Ok(r) => match (r.ret, r.picture) {
                (Some(v), Some(p)) if v >= 0 => planes_to_file(p.width, p.height, f32::from_bits(p.ratio), p.fields, p.planes, c, orig),
                (Some(v), _) if v < 0 => Made::Refused(format!("answer {v}: {:?}", t.log.status)),
                (v, _) => Made::Fault(format!("answer {v:?} with no picture record")),
            },
        }
    }
    fn vtg(row: &Row, b: &[u8], c: &[u8], orig: &[u8]) -> Made {
        match vertigo::read_vertigo(Some(b), 0, b"vertigo2soft", row.input.as_bytes()) {
            Err(e) => Made::Fault(e.0.to_string()),
            Ok(r) => match r.picture {
                Some(p) if r.ret == 1 => {
                    let Some(fields) = p.fields else { return Made::Fault("fields left as the caller had them".into()) };
                    let planes = p.planes.map(|q| q.flatten());
                    planes_to_file(p.width, p.height, p.ratio, fields, planes, c, orig)
                }
                _ => Made::Refused(format!("answer {}: {:?}", r.ret, r.messages)),
            },
        }
    }
    fn pct(b: &[u8], c: &[u8], mac: i32, orig: &[u8]) -> Made {
        match pict::read_pict(Some(b), mac) {
            Err(e) => Made::Fault(e.0.to_string()),
            Ok(r) => match r.picture {
                Some(p) if r.ret == 1 => planes_to_file(p.width, p.height, p.ratio, p.fields, p.planes, c, orig),
                _ => Made::Refused(format!("answer {:?}: {:?}", r.ret, r.messages)),
            },
        }
    }
    vec![
        ("alias2soft", "default", "SI_pictureImportAlias", |_, b, _, _, c, _| match alias::import_alias(b, Some(c), false) {
            Ok(x) => Made::Files(vec![(String::new(), masked(&x, &[102..104]))]),
            Err(e) => Made::Refused(format!("{e:?}")),
        }),
        ("aurora2soft", "default", "SI_pictureImportAurora", |_, b, _, _, c, _| match aurora::import_aurora(b, Some(c), false) {
            Ok(x) => Made::Files(vec![(String::new(), masked(&x, &[102..104]))]),
            Err(e) => Made::Refused(format!("{e:?}")),
        }),
        ("tga2soft", "default", "SI_pictureReadTarga (quiet, alpha as stored, ratio 1), SI_pictureWrite", |_, b, _, _, c, _| tga(b, c)),
        ("tga2soft", "R", "SI_pictureReadTarga (quiet, alpha as stored, ratio 1), SI_pictureWrite", |_, b, _, _, c, _| tga(b, c)),
        ("sgi2soft", "default", "SI_pictureReadSgi, SI_pictureWrite", |_, b, _, _, c, _| match sgi::picture_read_sgi(Some(b)) {
            Ok(Ok((p, _))) => pic_file(&p, c),
            Ok(Err(e)) => Made::Refused(format!("{e:?}")),
            Err(e) => Made::Fault(format!("{e:?}")),
        }),
        ("yuv2soft", "default", "SI_pictureReadYuv, SI_pictureWrite", |_, b, _, _, c, _| match yuv::read_yuv(b, None) {
            Ok((p, _)) => pic_file(&p, c),
            Err(e) => Made::Fault(format!("{e:?}")),
        }),
        ("raw2soft", "default", "SI_pictureReadRaw's rule (the main reads the planes itself), SI_pictureWrite", |_, _, (w, h), files, c, _| {
            let get = |e: &str| files.get(e).map(|v| &v[..]);
            match raw::read_raw(w, h, raw::RawSource::Files([get(".r"), get(".g"), get(".b"), get(".a")])) {
                Ok(p) => pic_file(&p, c),
                Err(e) => Made::Refused(format!("{e:?}")),
            }
        }),
        ("rgb2soft", "d", "SI_pictureReadRgb's rule (the main reads the file itself), SI_pictureWrite", |r, b, (w, h), _, c, _| {
            let p = if r.input.ends_with(".rgba") { raw::read_rgb(w, h, true, None, Some(b)) } else { raw::read_rgb(w, h, false, Some(b), None) };
            match p {
                Ok(p) => pic_file(&p, c),
                Err(e) => Made::Refused(format!("{e:?}")),
            }
        }),
        ("rgb2soft", "a-d", "SI_pictureReadRgb's rule, no alpha, SI_pictureWrite", |_, b, (w, h), _, c, _| match raw::read_rgb(w, h, false, Some(b), None) {
            Ok(p) => pic_file(&p, c),
            Err(e) => Made::Refused(format!("{e:?}")),
        }),
        ("tiff2soft", "default", "SI_pictureReadTiff, SI_pictureWrite", |r, b, _, _, c, o| tif(r, b, c, 0, o)),
        ("tiff2soft", "R", "SI_pictureReadTiff, SI_pictureWrite", |r, b, _, _, c, o| tif(r, b, c, 0, o)),
        ("tiff2soft", "p", "SI_pictureReadTiff, SI_pictureWrite", |r, b, _, _, c, o| tif(r, b, c, 0, o)),
        ("tiff2soft", "m", "SI_pictureReadTiff (MacBinary 0x80), SI_pictureWrite", |r, b, _, _, c, o| tif(r, b, c, 0x80, o)),
        ("tiff2soft", "m-without-m", "SI_pictureReadTiff (a MacBinary file read as plain), SI_pictureWrite", |r, b, _, _, c, o| tif(r, b, c, 0, o)),
        ("everest2soft", "default", "SI_pictureImportEverest", |_, b, _, _, c, _| match everest::import_everest(b, c) {
            Ok(everest::Imported::File(x)) => Made::Files(vec![(String::new(), x)]),
            Ok(everest::Imported::CannotRead) => Made::Refused("cannot read".into()),
            Err(e) => Made::Fault(e.0.to_string()),
        }),
        ("vertigo2soft", "default", "SI_pictureReadVertigo (whole frame), SI_pictureWrite", |r, b, _, _, c, o| vtg(r, b, c, o)),
        ("vertigo2soft", "p", "SI_pictureReadVertigo (whole frame), SI_pictureWrite", |r, b, _, _, c, o| vtg(r, b, c, o)),
        ("pict2soft", "default", "SI_pictureReadPict, SI_pictureWrite", |_, b, _, _, c, o| pct(b, c, 0, o)),
        ("pict2soft", "m", "SI_pictureReadPict (MacBinary 0x80), SI_pictureWrite", |_, b, _, _, c, o| pct(b, c, 0x80, o)),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tools = PathBuf::from(args.get(1).expect("usage: imgfmt-oracle <re/out/oracle/tools> [--keep <directory>]"));
    let keep = args.iter().position(|a| a == "--keep").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let only: Option<&String> = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1));
    let mut cx = Ctx { tools: tools.clone(), keep, out: String::new() };
    let mut dims: BTreeMap<String, (i16, i16)> = BTreeMap::new();
    for l in fs::read_to_string(tools.join("inputs/sample.tsv")).expect("sample.tsv").lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        if let (Ok(w), Ok(h)) = (f[3].parse(), f[4].parse()) {
            dims.insert(f[0].to_string(), (w, h));
        }
    }
    let lines_dir = cx.keep.clone();

    for (tool, variant, how, f) in from_table() {
        if only.is_some_and(|o| !tool.contains(o.as_str())) {
            continue;
        }
        let dir = cx.tools.join(tool).join(variant);
        let rdir = cx.tools.join(tool).join(format!("{variant}-repeat"));
        let rrows = manifest(&rdir);
        let mut t = Tally::default();
        let rows = manifest(&dir);
        if rows.is_empty() {
            continue;
        }
        for row in &rows {
            let pid = row.id.rsplit('.').next().unwrap_or("").to_string();
            let prefix = format!("{tool}.{variant}.{pid}");
            let input = fs::read(cx.tools.join(&row.input)).unwrap_or_default();
            let orig = originals(&dir, &prefix, row);
            let first = orig.first().map(|x| x.1.clone()).unwrap_or_default();
            let made = match read_pic(&input) {
                Ok(p) => f(row, &input, &p, &first),
                Err(e) => Made::Refused(e),
            };
            let rrow = rrows.iter().find(|r| r.id.ends_with(&format!(".{pid}")));
            let rep = rrow.map(|r| originals(&rdir, &format!("{tool}.{variant}-repeat.{pid}"), r));
            // the port again, on the repeated command's own output name and date
            let rep_port: Vec<Vec<Option<u8>>> = match (rrow, &rep) {
                (Some(r), Some(ro)) => {
                    let rfirst = ro.first().map(|x| x.1.clone()).unwrap_or_default();
                    match read_pic(&input).map(|p| f(r, &input, &p, &rfirst)) {
                        Ok(Made::Files(v)) => v.into_iter().map(|x| x.1).collect(),
                        _ => Vec::new(),
                    }
                }
                _ => Vec::new(),
            };
            let repeat = rep.as_deref().map(|r| (r, rep_port));
            judge(&mut cx, &mut t, &row.id, made, &orig, repeat, &row.status);
        }
        let _ = writeln!(cx.out, "{tool:<13} {variant:<12} [{how}]\n    {}", t.summary());
        if let Some(d) = &lines_dir {
            let _ = fs::create_dir_all(d);
            let _ = fs::write(d.join(format!("{tool}.{variant}.tsv")), &t.lines);
        }
    }

    for (tool, variant, how, f) in back_table() {
        if only.is_some_and(|o| !tool.contains(o.as_str())) {
            continue;
        }
        let dir = cx.tools.join(tool).join(variant);
        let mut t = Tally::default();
        let rows = manifest(&dir);
        if rows.is_empty() {
            continue;
        }
        for row in &rows {
            let pid = row.id.rsplit('.').next().unwrap_or("").to_string();
            let orig = originals(&dir, &row.id, row);
            let first = orig.first().map(|x| x.1.clone()).unwrap_or_default();
            let comment = comment_of(&first);
            let input_path = cx.tools.join(&row.input);
            let input = fs::read(&input_path).unwrap_or_default();
            let mut files = BTreeMap::new();
            for e in raw::RAW_EXTENSIONS {
                let mut p = input_path.clone().into_os_string();
                p.push(e);
                if let Ok(b) = fs::read(&p) {
                    files.insert(e.to_string(), b);
                }
            }
            let d = dims.get(&pid).copied().unwrap_or((0, 0));
            let made = f(row, &input, d, &files, &comment, &first);
            // the original's round trip against the sample picture, and how much of what it
            // changed the port changes the same way
            let sample = fs::read(cx.tools.join(format!("inputs/pic/{pid}.pic"))).ok().and_then(|b| read_pic(&b).ok());
            if let (Some(sp), Ok(op)) = (&sample, read_pic(&first)) {
                let port_planes: Option<Vec<(usize, Vec<Option<u8>>)>> = match &made {
                    Made::Files(v) if v.len() == 2 && v[0].0 == "#planes" => {
                        // the planes one after another, for the planes both have
                        let mut out = Vec::new();
                        let mut at = 0usize;
                        for c in 0..4 {
                            if let (Some(q), true) = (&op.planes[c], at < v[0].1.len()) {
                                if at + q.len() <= v[0].1.len() {
                                    out.push((c, v[0].1[at..at + q.len()].to_vec()));
                                    at += q.len();
                                }
                            }
                        }
                        Some(out)
                    }
                    Made::Files(v) if v.len() == 1 => {
                        let bytes: Vec<u8> = v[0].1.iter().map(|b| b.unwrap_or(0)).collect();
                        read_pic(&bytes).ok().map(|pp| (0..4).filter_map(|c| pp.planes[c].clone().map(|q| (c, defined(&q)))).collect())
                    }
                    _ => None,
                };
                if let Some(pp) = port_planes {
                    if (sp.width, sp.height) == (op.width, op.height) {
                        for (c, port) in pp {
                            let (Some(sq), Some(oq)) = (&sp.planes[c], &op.planes[c]) else { continue };
                            for i in 0..oq.len().min(sq.len()).min(port.len()) {
                                if oq[i] != sq[i] {
                                    t.rt_changed += 1;
                                    match port[i] {
                                        Some(b) if b == oq[i] => t.rt_same += 1,
                                        None => t.rt_undef += 1,
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // the planes comparison (undefined pixels in the port) is judged on its own files
            let made = match made {
                Made::Files(v) if v.len() == 2 && v[0].0 == "#planes" => {
                    let theirs: Vec<u8> = v[1].1.iter().map(|b| b.unwrap()).collect();
                    let (n, _, undef) = compare(&v[0].1, &theirs);
                    let class = if n == 0 { "same planes on the defined pixels" } else { "planes differ on defined pixels" };
                    let st = if row.status == "139" { " (original: segmentation fault after writing)" } else { "" };
                    t.add(&row.id, &format!("{class}{st}"), &format!("{n} differ, {undef} undefined in the port"));
                    continue;
                }
                m => m,
            };
            judge(&mut cx, &mut t, &row.id, made, &orig, None, &row.status);
        }
        let _ = writeln!(cx.out, "{tool:<13} {variant:<12} [{how}]\n    {}", t.summary());
        if t.rt_changed > 0 {
            let _ = writeln!(cx.out, "    the original's round trip changes {} samples of the sample pictures; the port changes {} the same way, {} undefined", t.rt_changed, t.rt_same, t.rt_undef);
        }
        if let Some(d) = &lines_dir {
            let _ = fs::create_dir_all(d);
            let _ = fs::write(d.join(format!("{tool}.{variant}.tsv")), &t.lines);
        }
    }
    print!("{}", cx.out);
}
