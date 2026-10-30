// SPDX-License-Identifier: MIT
//! The image classes against `imgconv` (finding 0014, sixth pass, "The image classes";
//! outputs: finding 0031, third pass).
//!
//!   (ulimit -v 2000000; imgfmt-imgconv re/out/oracle/tools [--mutate])
//!
//! `imgconv` links the image classes (`ilImage*.c++`) as `soft` has them: the same-code scan
//! (`re/tools/si-imgfmt/samecode.py --inline imgconv`) finds all 140 library functions it
//! defines the same, instruction for instruction. Its formats and processors are plug-ins
//! (`dso/sil/ilImage<NAME>.so`) that are not `soft`'s code. What `soft`'s classes decide in an
//! `imgconv` run is the colour reduction: for a colour-mapped output format `imgconv`'s `main`
//! (imgconv 0x0804e3c0) makes an `SI_ImageCMAP` of 256 colours (or `-reduction` colours),
//! fills its colours with `generateColorIndex` from the image read and maps the image to
//! indices with `writeTile`; the plug-in then writes the colours and indices. Two formats
//! show them:
//!
//! - **DGT** (`to-dgt`, 68 pictures): the plug-in's writer (`writeNextLine`, ilImageDGT.so
//!   0x49d4, read from its instructions) is a few lines and is reproduced here, uncited (it is
//!   not `soft`'s): a header, every colour at 5 bits a channel, every index. The whole file is
//!   compared byte for byte.
//! - **TIM with `-reduction 256`** (`to-TIM-r256`, 39 pictures): the plug-in prints every
//!   colour of the map at 8 bits (`Pixel : r g b a : tim`), and the file holds the indices,
//!   one byte each. The printed lines are compared whole, the indices by row.
//!
//! The input picture is read by the port's picture reader (the SOFT plug-in reads the file to
//! the same pixels: `imgconv`'s `to-pic` round trips give every sample back) and put into an
//! `SI_ImageABGR` as the SOFT plug-in does: its orientation is 0, so the file's first row is
//! the image's last line.
//!
//! `--mutate` drops the median cut's "box number modulo 4" from the colours: the check must
//! then fail.

use std::fs;
use std::path::Path;

use si_imgfmt::image::{ImageAbgr, ImageBase, ImageCmap, ImageMem};
use si_picture::Picture;

struct Row {
    id: String,
    input: String,
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
            Row { id: f[0].to_string(), input: f[1].to_string(), status: f.get(3).unwrap_or(&"").to_string(), outputs }
        })
        .collect()
}

/// The image `imgconv` holds after reading a picture file through the SOFT plug-in.
fn abgr_of(p: &Picture) -> ImageAbgr {
    let (w, h) = (p.width as i32, p.height as i32);
    let mut img = ImageAbgr::new(w, h);
    for row in 0..h {
        let line = h - 1 - row;
        for x in 0..w {
            let at = (row * w + x) as usize;
            let s = |c: usize| p.planes[c].as_ref().map_or(0, |v| v[at]);
            let a = p.planes[3].as_ref().map_or(0xff, |v| v[at]);
            img.set_pixel(x, line, [s(0), s(1), s(2), a]).unwrap();
        }
    }
    img
}

/// What `imgconv`'s `main` does for a colour-mapped output: an `SI_ImageCMAP` of `colours`
/// colours, `generateColorIndex`, `writeTile` of the whole image.
fn reduce(img: &mut ImageAbgr, colours: i32, mutate: bool) -> Result<ImageCmap, String> {
    let (w, h) = (img.width(), img.height());
    let mut cmap = ImageCmap::new(colours, w, h);
    cmap.generate_color_index(img).map_err(|f| f.0.to_string())?;
    if mutate {
        // the colours without the box number's last two bits (only boxes of more than one
        // cell carry them; changing every colour to its multiple of 4 is near enough)
        for c in 0..3 {
            if let Some(t) = cmap.tables[c].as_mut() {
                for v in t.iter_mut() {
                    *v &= 0xfc;
                }
            }
        }
    }
    cmap.write_tile(0, 0, w, h, img, 0, 0).map_err(|f| f.0.to_string())?;
    Ok(cmap)
}

/// The DGT plug-in's file for a colour-mapped image (writeNextLine, ilImageDGT.so 0x49d4,
/// with `SI_ImageFile::writeImage` and the plug-in's orientation 0: lines last to first; the
/// header and the colours go out with the first line written). Not `soft`'s code.
fn dgt_plugin_file(cmap: &ImageCmap, description: &[u8]) -> Vec<u8> {
    let (w, h) = (cmap.width(), cmap.height());
    let n = cmap.index_max_value() + 1;
    let mut f = Vec::new();
    f.extend_from_slice(&0x10000u32.to_le_bytes()); // the word 0x10000 written as it is
    f.extend_from_slice(&1u32.to_be_bytes()); // number of images
    f.extend_from_slice(&((0x120 + 2 * w * h + 4 * n) as u32).to_be_bytes());
    f.extend_from_slice(&[0; 4]); // skipped by fseek: a hole of zeros
    f.extend_from_slice(b"DGT             ");
    f.resize(0x100, 0); // fseek 0xe0
    let colours = (n as u32).min(0x10000);
    f.extend_from_slice(&(0x20 + 4 * colours).to_be_bytes());
    f.extend_from_slice(&[0; 4]);
    f.extend_from_slice(&(w as u16).to_be_bytes());
    f.extend_from_slice(&(h as u16).to_be_bytes());
    f.extend_from_slice(&[0; 4]);
    let mut d = [0u8; 16];
    for (i, &c) in description.iter().take(15).enumerate() {
        d[i] = c;
        if c == 0 {
            break;
        }
    }
    f.extend_from_slice(&d);
    for i in 0..colours {
        let [r, g, b] = cmap.get_color_index_value3(i as u16 as i32).unwrap().unwrap().map(|c| c >> 3);
        let v = (b as u16 & 1) << 14 | (b as u16 & 0x1e) << 7 | (g as u16 & 1) << 13 | (g as u16 & 0x1e) << 3 | (r as u16 & 1) << 12 | (r >> 1) as u16;
        f.extend_from_slice(&(i as u16).to_be_bytes());
        f.extend_from_slice(&v.to_be_bytes());
    }
    for line in (0..h).rev() {
        for x in 0..w {
            f.extend_from_slice(&(cmap.get_cpixel(x, line).unwrap() as u16).to_be_bytes());
        }
    }
    f
}

#[derive(Default)]
struct Count {
    identical: usize,
    differing: usize,
    both_refuse: usize,
    other: usize,
    notes: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(args.get(1).map(String::as_str).unwrap_or("re/out/oracle/tools"));
    let mutate = args.iter().any(|a| a == "--mutate");
    let conv = root.join("imgconv");

    // DGT
    let mut dgt = Count::default();
    let dir = conv.join("to-dgt");
    for row in manifest(&dir) {
        let input = fs::read(root.join(&row.input)).unwrap_or_default();
        let pic = Picture::read(&input);
        let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
        match (pic, theirs) {
            (Err(_), None) => dgt.both_refuse += 1,
            (Ok(pf), Some(theirs)) => {
                let mut img = abgr_of(&pf.picture);
                match reduce(&mut img, 256, mutate) {
                    Ok(cmap) => {
                        let comment: Vec<u8> = pf.comment.iter().take_while(|&&c| c != 0).copied().collect();
                        let ours = dgt_plugin_file(&cmap, &comment);
                        if ours == theirs {
                            dgt.identical += 1;
                        } else {
                            dgt.differing += 1;
                            let first = ours.iter().zip(&theirs).position(|(a, b)| a != b).unwrap_or(ours.len().min(theirs.len()));
                            dgt.notes.push(format!("{}: differs from byte {first} (sizes {} and {})", row.id, ours.len(), theirs.len()));
                        }
                    }
                    Err(e) => {
                        dgt.other += 1;
                        dgt.notes.push(format!("{}: the port stops: {e}", row.id));
                    }
                }
            }
            (p, t) => {
                dgt.other += 1;
                dgt.notes.push(format!("{}: port {}, imgconv {} (status {})", row.id, if p.is_ok() { "reads" } else { "refuses" }, if t.is_some() { "writes" } else { "writes nothing" }, row.status));
            }
        }
    }
    println!("imgconv to-dgt (SI_ImageCMAP::generateColorIndex, writeTile; the DGT plug-in's writer reproduced)");
    println!("  identical {}  differing {}  both refuse {}  other {}", dgt.identical, dgt.differing, dgt.both_refuse, dgt.other);
    for n in dgt.notes.iter().take(20) {
        println!("    {n}");
    }

    // TIM with -reduction 256: the printed colours and the indices
    let mut tim = Count::default();
    let mut tim_idx = Count::default();
    let mut tim_clut = Count::default();
    let dir = conv.join("to-TIM-r256");
    for row in manifest(&dir) {
        let input = fs::read(root.join(&row.input)).unwrap_or_default();
        let pic = Picture::read(&input);
        let printed = fs::read_to_string(dir.join("printed").join(format!("{}.txt", row.id))).unwrap_or_default();
        let theirs = row.outputs.first().and_then(|o| fs::read(dir.join(o)).ok());
        match (pic, theirs) {
            (Err(_), None) => {
                tim.both_refuse += 1;
                tim_idx.both_refuse += 1;
                tim_clut.both_refuse += 1;
            }
            (Ok(pf), Some(theirs)) => {
                let mut img = abgr_of(&pf.picture);
                let cmap = match reduce(&mut img, 256, mutate) {
                    Ok(c) => c,
                    Err(e) => {
                        tim.other += 1;
                        tim.notes.push(format!("{}: the port stops: {e}", row.id));
                        continue;
                    }
                };
                let mut lines = String::new();
                for i in 0..256 {
                    let [r, g, b, a] = cmap.get_color_index_value4(i).unwrap().unwrap();
                    let v = (r >> 3) as u32 | ((g >> 3) as u32) << 5 | ((b >> 3) as u32) << 10;
                    lines.push_str(&format!("Pixel : {r} {g} {b} {a} : {v}\n"));
                }
                if printed.is_empty() {
                    tim.other += 1;
                    tim.notes.push(format!("{}: imgconv printed nothing (status {}); see the colour block", row.id, row.status));
                } else if lines == printed {
                    tim.identical += 1;
                } else {
                    tim.differing += 1;
                    let k = lines.lines().zip(printed.lines()).position(|(a, b)| a != b).unwrap_or(0);
                    tim.notes.push(format!("{}: colour {k}: port `{}`, imgconv `{}`", row.id, lines.lines().nth(k).unwrap_or(""), printed.lines().nth(k).unwrap_or("")));
                }
                // the file (big-endian 32-bit words): an 8-byte header, the colour block (its
                // size, four words of place and size, 256 16-bit colours), the image block
                // (its size, four words, the indices in 32-bit words of four, byte-reversed),
                // rows of `2 × ceil(w / 2)` bytes, the picture file's first row first (the
                // image's last line) [observed]
                let (w, h) = (img.width(), img.height());
                let be32 = |at: usize| theirs.get(at..at + 4).map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap()) as usize);
                let clut = be32(8);
                let mut same_clut = true;
                for i in 0..256usize {
                    let v = theirs.get(20 + 2 * i..22 + 2 * i).map(|b| u16::from_be_bytes([b[0], b[1]]));
                    let [r, g, b] = cmap.get_color_index_value3(i as i32).unwrap().unwrap();
                    let ours = (r >> 3) as u16 | ((g >> 3) as u16) << 5 | ((b >> 3) as u16) << 10;
                    if v != Some(ours) {
                        same_clut = false;
                    }
                }
                if same_clut {
                    tim_clut.identical += 1;
                } else {
                    tim_clut.differing += 1;
                    tim_clut.notes.push(format!("{}: the colour block differs", row.id));
                }
                let at = 8 + clut;
                let size = be32(at);
                let body: Vec<u8> = theirs.get(at + 12..(at + size).min(theirs.len())).unwrap_or(&[]).chunks(4).flat_map(|c| c.iter().rev().copied().collect::<Vec<u8>>()).collect();
                let stride = (w as usize + 1) & !1;
                let mut same = true;
                let mut missing = 0;
                for k in 0..h {
                    for x in 0..w {
                        match body.get(k as usize * stride + x as usize).copied() {
                            // the plug-in writes whole 32-bit words: a last part word is
                            // left out of the file
                            None => missing += 1,
                            Some(v) => {
                                if v != cmap.get_cpixel(x, h - 1 - k).unwrap() as u8 {
                                    same = false;
                                }
                            }
                        }
                    }
                }
                if same && missing > 0 {
                    tim_idx.other += 1;
                    tim_idx.notes.push(format!("{}: identical on the {} indices the file holds; {missing} left out (a last part word)", row.id, w * h - missing));
                    continue;
                }
                if same {
                    tim_idx.identical += 1;
                } else {
                    tim_idx.differing += 1;
                    tim_idx.notes.push(format!("{}: indices differ", row.id));
                }
            }
            (p, t) => {
                tim.other += 1;
                tim.notes.push(format!("{}: port {}, imgconv {} (status {})", row.id, if p.is_ok() { "reads" } else { "refuses" }, if t.is_some() { "writes" } else { "writes nothing" }, row.status));
            }
        }
    }
    println!("imgconv to-TIM -reduction 256: the colour map as the plug-in prints it");
    println!("  identical {}  differing {}  both refuse {}  other {}", tim.identical, tim.differing, tim.both_refuse, tim.other);
    for n in tim.notes.iter().take(20) {
        println!("    {n}");
    }
    println!("imgconv to-TIM -reduction 256: the colour block in the file (5 bits a channel)");
    println!("  identical {}  differing {}  both refuse {}", tim_clut.identical, tim_clut.differing, tim_clut.both_refuse);
    for n in tim_clut.notes.iter().take(20) {
        println!("    {n}");
    }
    println!("imgconv to-TIM -reduction 256: the indices in the file");
    println!("  identical {}  differing {}  both refuse {}  identical where the file holds them {}", tim_idx.identical, tim_idx.differing, tim_idx.both_refuse, tim_idx.other);
    for n in tim_idx.notes.iter().take(20) {
        println!("    {n}");
    }
}
