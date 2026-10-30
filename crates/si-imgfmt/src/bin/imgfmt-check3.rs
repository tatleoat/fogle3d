// SPDX-License-Identifier: MIT
//! The corpus pass of the third round over the picture library: SGI, YUV, `.dgt` and `.pal`
//! files, and colour reduction from end to end.
//!
//!   (ulimit -v 2000000; imgfmt-check3 <directory> [more directories])
//!
//! First table: every file under the directories is looked at **by content** (not by its
//! name) for the formats that can be told by content, and each one found is read by the
//! port, written back and compared. Second table: every picture file (`.pic`) goes through
//! the writers and readers and through the colour reduction, and invariants are counted.
//! The second table is the port against itself; the comparison with the original is
//! `imgfmt-emucheck`.

use si_imgfmt::dgt::{self, DgPicture};
use si_imgfmt::reduce::{QuantizeParameters, Reducer};
use si_imgfmt::sgi::{self, Written, HEADER_UNDEFINED};
use si_imgfmt::yuv;
use si_picture::{Picture, PictureRgba};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            walk(&p, out);
        } else if meta.is_file() {
            out.push(p);
        }
    }
}

#[derive(Default)]
struct Row {
    yes: usize,
    no: usize,
    na: usize,
}

struct Table(BTreeMap<&'static str, Row>, Vec<&'static str>);

impl Table {
    fn add(&mut self, name: &'static str, r: Option<bool>) {
        if !self.0.contains_key(name) {
            self.1.push(name);
        }
        let row = self.0.entry(name).or_default();
        match r {
            Some(true) => row.yes += 1,
            Some(false) => row.no += 1,
            None => row.na += 1,
        }
    }

    fn print(&self, title: &str) {
        println!("{:<58}{:>8}{:>10}{:>16}", title, "holds", "does not", "not applicable");
        for name in &self.1 {
            let r = &self.0[name];
            println!("{:<58}{:>8}{:>10}{:>16}", name, r.yes, r.no, r.na);
        }
    }
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// One SGI file of the corpus: read, written back, compared.
fn sgi_file(t: &mut Table, path: &Path, data: &[u8]) {
    let read = sgi::picture_read_sgi(Some(data));
    let Ok(Ok((p, errors))) = read else {
        t.add("SGI: read by the port", Some(false));
        println!("  not read: {} ({:?})", path.display(), read.map(|r| r.map(|_| ())));
        return;
    };
    t.add("SGI: read by the port", Some(true));
    t.add("SGI: read without a message from the library", Some(errors.is_empty()));
    let big_endian = data[0] == 1;
    let (w, h, z) = (p.width as usize, p.height as usize, if p.planes[3].is_some() { 4 } else { 3 });
    println!("  {}: {} x {}, {} planes in the picture, kind {:02x}{:02x}, {}", path.display(), w, h, z, data[2], data[3], if big_endian { "big-endian" } else { "byte-reversed" });
    let Ok(Written::File(back)) = sgi::picture_write_sgi(&p) else {
        t.add("SGI: written back", Some(false));
        return;
    };
    t.add("SGI: written back, read again: the same picture", sgi::picture_read_sgi(Some(&back)).ok().and_then(|r| r.ok()).map(|(q, _)| q.planes == p.planes));
    t.add("SGI: written back: byte for byte", Some(back == data));
    // The port writes what this build writes: a little-endian header and tables. Compared
    // as numbers, and the row data byte for byte, the file may still be the same.
    let n = h * z;
    let rle = data[2] == 1 && data[3] == 1;
    let file_planes = u16::from_be_bytes([data[10], data[11]]) as usize;
    if big_endian && rle && file_planes == z && data.len() >= 0x200 + 8 * n {
        let same_tables = (0..2 * n).all(|k| be32(data, 0x200 + 4 * k) == le32(&back, 0x200 + 4 * k));
        t.add("SGI: row tables the same as numbers", Some(same_tables));
        t.add("SGI: row data byte for byte", Some(data[0x200 + 8 * n..] == back[0x200 + 8 * n..]));
        let fields = (0..6).all(|k| u16::from_be_bytes([data[2 * k], data[2 * k + 1]]) == u16::from_le_bytes([back[2 * k], back[2 * k + 1]]));
        t.add("SGI: magic, kind, dimensions, sizes the same as numbers", Some(fields));
        t.add("SGI: least and greatest sample the same as numbers", Some(be32(data, 12) == le32(&back, 12) && be32(data, 16) == le32(&back, 16)));
        let _ = HEADER_UNDEFINED;
    } else {
        t.add("SGI: row tables the same as numbers", None);
        t.add("SGI: row data byte for byte", None);
    }
}

fn corner(p: &Picture, side: usize) -> Picture {
    let (w, h) = (p.width as usize, p.height as usize);
    let (cw, ch) = (w.min(side), h.min(side));
    let cut = |pl: &Option<Vec<u8>>| pl.as_ref().map(|v| (0..ch).flat_map(|y| v[y * w..y * w + cw].iter().copied()).collect::<Vec<u8>>());
    Picture { width: cw as i16, height: ch as i16, planes: [cut(&p.planes[0]), cut(&p.planes[1]), cut(&p.planes[2]), cut(&p.planes[3])], ..p.clone() }
}

fn packed(p: &Picture) -> PictureRgba {
    let n = p.width as usize * p.height as usize;
    let mut data = Vec::with_capacity(4 * n);
    for i in 0..n {
        for c in 0..4 {
            data.push(p.planes[c].as_ref().map_or(0xff, |v| v[i]));
        }
    }
    PictureRgba { width: p.width, height: p.height, ratio: p.ratio, fields: p.fields, pad: 0, data }
}

fn reduce(pic: &PictureRgba, colors: i32, q: QuantizeParameters) -> Option<(DgPicture, Reducer)> {
    let mut r = Reducer::new(false);
    let mut bits = [8; 4];
    let (dg, _) = r.color_reduce(pic, colors, &mut bits, None, 0, 0, q, false).ok()?;
    Some((dg?, r))
}

/// Invariants of colour reduction and of the index picture's files on one picture.
fn picture_file(t: &mut Table, p: &Picture) {
    let c = corner(p, 48);
    let n = c.width as usize * c.height as usize;
    let pic = packed(&c);
    let colours: std::collections::BTreeSet<[u8; 3]> = pic.data.chunks_exact(4).map(|x| [x[0], x[1], x[2]]).collect();
    let methods = [
        ("reduce to 16, median boxes", QuantizeParameters { method: 2, quality: 1, cut: 1, representative: 3 }),
        ("reduce to 16, variance boxes", QuantizeParameters { method: 2, quality: 1, cut: 2, representative: 3 }),
        ("reduce to 16, neural", QuantizeParameters { method: 1, quality: 1, cut: 1, representative: 3 }),
    ];
    let mut first = None;
    for (k, (name, q)) in methods.into_iter().enumerate() {
        let made = reduce(&pic, 16, q);
        let names: [&'static str; 3] = [
            ["median boxes, 16 colours: indices inside the palette, counts add up", "variance boxes, 16 colours: the same", "neural, 16 colours: the same"][k],
            ["median boxes: a picture of at most 16 colours comes back exactly", "variance boxes: the same", "neural: the same"][k],
            name,
        ];
        let Some((dg, r)) = made else {
            t.add(names[0], Some(false));
            continue;
        };
        let pal = dg.palette.as_ref().unwrap();
        let ix = dg.data.as_ref().unwrap();
        let counted: u32 = r.count_reduce.as_ref().map_or(0, |v| v.iter().map(|e| e[1]).sum());
        t.add(names[0], Some(ix.iter().all(|&i| (i as u32) < pal.count) && counted as usize == n && pal.count == 16));
        let exact = ix.iter().zip(pic.data.chunks_exact(4)).all(|(&i, px)| pal.entries[i as usize].color[..3] == px[..3]);
        t.add(names[1], (colours.len() <= 16).then_some(exact));
        if k == 0 {
            first = Some(dg);
        }
    }
    if let Some(dg) = first {
        // the two files of an index picture
        let pal = dg.palette.as_ref().unwrap();
        let text = dgt::write_user_pal(pal).ok();
        let back = text.as_ref().and_then(|b| dgt::read_user_pal(Some(b)).ok().flatten());
        t.add("text palette written and read: the same colours", back.map(|b| b.entries.iter().map(|e| e.color).eq(pal.entries.iter().map(|e| e.color))));
        let file = dgt::write_dgt_file(&dg).ok();
        let back = file.as_ref().and_then(|b| dgt::read_dgt_file(b"/x.dgt", Some(b), 1).ok().flatten());
        t.add(
            ".dgt written and read: the indices, upside down",
            back.as_ref().map(|b| {
                let (w, h) = (dg.width as usize, dg.height as usize);
                let (a, o) = (b.data.as_ref().unwrap(), dg.data.as_ref().unwrap());
                (0..h).all(|y| a[w * y..w * (y + 1)] == o[w * (h - 1 - y)..w * (h - y)])
            }),
        );
        t.add(
            ".dgt written and read: the palette at 5 bits",
            back.map(|b| {
                let bp = b.palette.unwrap();
                pal.entries.iter().zip(&bp.entries).all(|(e, f)| (0..3).all(|c| f.color[c] as i32 == ((e.color[c] as f32) * (31.0f32 / 255.0) + 0.5) as i32))
            }),
        );
    }
    // SGI: run-length, one byte a sample; everything comes back
    let sgi_back = match sgi::picture_write_sgi(&c) {
        Ok(Written::File(b)) => sgi::picture_read_sgi(Some(&b)).ok().and_then(|r| r.ok()).map(|(q, _)| q.planes == c.planes && (q.width, q.height) == (c.width, c.height)),
        _ => None,
    };
    t.add("SGI written and read: the same picture", sgi_back);
    // YUV: not a round trip (colour is halved and limited); the frame and the centring
    let frame = yuv::write_yuv(&c, false, None).ok().flatten();
    t.add(
        "YUV written: an NTSC frame; read: 720 x 486, black around the picture",
        frame.and_then(|f| yuv::read_yuv(&f, None).ok().map(|(q, pairs)| f.len() == 486 * 1440 && (q.width, q.height) == (720, 486) && pairs == 486 * 360 && q.planes[0].as_ref().unwrap()[0] == 0)),
    );
}

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        eprintln!("usage: imgfmt-check3 <directory> [more directories]");
        std::process::exit(2);
    }
    let mut files = Vec::new();
    for d in &dirs {
        walk(Path::new(d), &mut files);
    }
    files.sort();
    let mut found = Table(BTreeMap::new(), Vec::new());
    let mut ops = Table(BTreeMap::new(), Vec::new());
    let (mut sgi_n, mut dgt_n, mut pal_n, mut yuv_n, mut pictures) = (0, 0, 0, 0, 0);
    for path in &files {
        let Ok(meta) = std::fs::metadata(path) else { continue };
        let len = meta.len();
        // by content: the first bytes, and for YUV the only thing there is, the length
        let mut head = [0u8; 64];
        let got = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        let head = &head[..got];
        let is_sgi = got >= 12 && (head[..2] == [1, 0xda] || head[..2] == [0xda, 1]);
        let is_dgt = got >= 32 && &head[16..32] == dgt::DGT_NOTE;
        let is_yuv = len == 486 * 1440 || len == 576 * 1440;
        let is_pal = {
            // "<number> <number>\n1.0\n0.0 0.0\n0\n"
            let text = String::from_utf8_lossy(head);
            let mut lines = text.split('\n');
            let first: Vec<&str> = lines.next().unwrap_or("").split(' ').collect();
            first.len() == 2 && first.iter().all(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())) && lines.next() == Some("1.0") && lines.next() == Some("0.0 0.0")
        };
        if is_sgi || is_dgt || is_pal || is_yuv {
            if len > 64 << 20 {
                continue;
            }
            let Ok(data) = std::fs::read(path) else { continue };
            if is_sgi {
                sgi_n += 1;
                sgi_file(&mut found, path, &data);
            }
            if is_dgt {
                dgt_n += 1;
                let read = dgt::read_dgt_file(b"/x", Some(&data), 1);
                found.add(".dgt: read by the port", Some(matches!(read, Ok(Some(_)))));
                println!("  {}", path.display());
            }
            if is_pal {
                pal_n += 1;
                let read = dgt::read_user_pal(Some(&data));
                let back = read.as_ref().ok().and_then(|p| p.as_ref()).and_then(|p| dgt::write_user_pal(p).ok());
                found.add("text palette: read by the port", Some(matches!(read, Ok(Some(_)))));
                found.add("text palette: written back byte for byte", back.map(|b| b == data));
                println!("  {}", path.display());
            }
            if is_yuv {
                yuv_n += 1;
                println!("  the length of a YUV frame: {}", path.display());
            }
        }
        if path.extension().is_some_and(|x| x == "pic") {
            let Ok(data) = std::fs::read(path) else { continue };
            let Ok(f) = Picture::read(&data) else { continue };
            if f.picture.planes[..3].iter().any(|x| x.is_none()) || f.picture.width <= 0 || f.picture.height <= 0 {
                continue;
            }
            pictures += 1;
            picture_file(&mut ops, &f.picture);
        }
    }
    println!("{} files looked at by content: {} SGI, {} .dgt, {} text palettes, {} of a YUV frame's length", files.len(), sgi_n, dgt_n, pal_n, yuv_n);
    found.print("files found by content");
    println!();
    println!("{pictures} picture files with red, green and blue (the top left 48 x 48 pixels of each)");
    ops.print("port against itself");
    let bad = ops.0.values().any(|r| r.no != 0) || found.0.iter().any(|(k, r)| r.no != 0 && !k.contains("byte for byte") && !k.contains("least and greatest"));
    std::process::exit(if bad { 1 } else { 0 });
}
