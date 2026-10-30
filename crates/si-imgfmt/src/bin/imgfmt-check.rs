// SPDX-License-Identifier: MIT
//! Runs the foreign-format writers and readers and the in-place operations over every
//! picture file under a directory and reports counts.
//!
//!   (ulimit -v 2000000; imgfmt-check <directory>)
//!
//! No shipped file is in any of these formats, so what can be counted on the corpus is
//! whether each format gives a picture back: written by the port, read by the port, compared
//! with what the format can hold (Alias has no alpha; the others keep all four planes).
//! That checks the port against itself, not against the original: the comparison with the
//! original is `imgfmt-emucheck`.
//!
//! A second table counts invariants of the resizing and compositing operations on the same
//! pictures (a zoom by 1 gives the picture back, the file and memory versions of an
//! operation agree, and so on): again the port against itself.

use si_imgfmt::{alias, aurora, composite, effect, raw, size, targa, zoom};
use si_picture::Picture;
use std::path::Path;

#[derive(Default)]
struct Tally {
    same: usize,
    different: usize,
    refused: usize,
}

impl Tally {
    fn add(&mut self, r: Option<bool>) {
        match r {
            Some(true) => self.same += 1,
            Some(false) => self.different += 1,
            None => self.refused += 1,
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            walk(&p, out);
        } else if meta.is_file() && p.extension().is_some_and(|x| x == "pic") {
            out.push(p);
        }
    }
}

/// The picture with the planes a format cannot hold removed or filled as its reader fills them.
fn rgb_only(p: &Picture) -> Picture {
    Picture { planes: [p.planes[0].clone(), p.planes[1].clone(), p.planes[2].clone(), None], ..p.clone() }
}

fn same_planes(a: &Picture, b: &Picture) -> bool {
    (a.width, a.height) == (b.width, b.height) && a.planes == b.planes
}

/// The top left corner of a picture, at most `side` pixels each way: the resizers that work
/// in the software x87 unit are run on this, so that a pass over the corpus takes minutes.
fn corner(p: &Picture, side: usize) -> Picture {
    let (w, h) = (p.width as usize, p.height as usize);
    let (cw, ch) = (w.min(side), h.min(side));
    let cut = |pl: &Option<Vec<u8>>| pl.as_ref().map(|v| (0..ch).flat_map(|y| v[y * w..y * w + cw].iter().copied()).collect::<Vec<u8>>());
    Picture { width: cw as i16, height: ch as i16, planes: [cut(&p.planes[0]), cut(&p.planes[1]), cut(&p.planes[2]), cut(&p.planes[3])], ..p.clone() }
}

/// The invariants of the resizing and compositing operations on one picture: each entry is
/// (row name, held / did not hold / not applicable). The port against itself.
fn operations(p: &Picture, data: &[u8]) -> Vec<(&'static str, Option<bool>)> {
    let mut out = Vec::new();
    let ok = |r: Result<bool, effect::Fault>| r.ok().filter(|&done| done);
    let run = |q: &Picture, op: &dyn Fn(&mut Picture) -> Result<bool, effect::Fault>| {
        let mut t = q.clone();
        ok(op(&mut t)).map(|_| t)
    };
    out.push(("average by 1 x 1", run(p, &|t| size::average(t, 1, 1)).map(|t| same_planes(&t, p))));
    out.push(("enlarge by 1 x 1", run(p, &|t| size::enlarge(t, 1, 1)).map(|t| same_planes(&t, p))));
    out.push((
        "enlarge 2 x 3, average back",
        run(p, &|t| Ok(size::enlarge(t, 2, 3)? && size::average(t, 2, 3)?)).map(|t| same_planes(&t, p)),
    ));
    let c = corner(p, 48);
    out.push(("bilinear zoom by 1", run(&c, &|t| size::bilinear_zoom(t, 1.0, 1.0)).map(|t| same_planes(&t, &c))));
    out.push(("square pixels, ratio 1", run(&c, &|t| size::scale_picture_x(t, 1.0)).map(|t| same_planes(&t, &c) && t.ratio == c.ratio)));
    // The area-weighted scale: every plane holds whole rows of the new width, and the new
    // width is the predicted one. Whether the record's height equals the rows made is
    // counted apart: the original computes the two from the factor before and after its
    // rounding to float, and they differ when height x factor + 0.5 is a whole number.
    let rows_made = |t: &Picture| t.planes.iter().flatten().map(|v| v.len()).next().filter(|_| t.width > 0).map(|n| n / t.width as usize);
    let whole_rows = |t: &Picture| t.width > 0 && t.planes.iter().flatten().all(|v| v.len() % t.width as usize == 0 && Some(v.len() / t.width as usize) == rows_made(t));
    for (a, b, z) in [("scale 0.5: whole rows", "scale 0.5: height = rows made", 0.5f32), ("scale 1.7: whole rows", "scale 1.7: height = rows made", 1.7)] {
        let t = run(&c, &|t| size::scale(t, z, z));
        out.push((a, t.as_ref().map(whole_rows)));
        out.push((b, t.as_ref().map(|t| rows_made(t) == Some(t.height as usize))));
    }
    // file to file: the whole-factor enlarger against the in-memory one, and the filtered
    // zoom's output as a picture file of the size and channels it should have
    let from_file = Picture::read(data).ok().map(|f| (f.picture, f.packets.len()));
    if let Some((whole, packets)) = from_file {
        let small = whole.width as usize * whole.height as usize <= 200_000;
        out.push((
            "enlarge on disk = in memory",
            if small { zoom::enlarge_disk(data, false, 2, 2, None).ok().and_then(|b| Picture::read(&b).ok()).and_then(|f| run(&whole, &|t| size::enlarge(t, 2, 2)).map(|t| same_planes(&f.picture, &t))) } else { None },
        ));
        out.push((
            "reduce on disk by 1 x 1",
            if small && whole.planes.iter().all(|x| x.is_some()) { zoom::reduce(data, false, 1, 1, None).ok().and_then(|b| Picture::read(&b).ok()).map(|f| same_planes(&f.picture, &whole)) } else { None },
        ));
        let file = c.write(None, false);
        for (name, z) in [("filter zoom 0.5: a picture", 0.5f32), ("filter zoom 1.5: a picture", 1.5)] {
            out.push((
                name,
                zoom::filter_zoom(&file, "o.pic", z, z, 1.0).ok().and_then(|b| Picture::read(&b).ok()).map(|f| {
                    let want = |v: i16| (v as f32 * z + 0.5) as i16;
                    (f.picture.width, f.picture.height) == (want(c.width), want(c.height)) && f.picture.planes[3].is_some() == c.planes[3].is_some()
                }),
            ));
        }
        let _ = packets;
    }
    // compositing: all four planes are needed
    let n = c.width as usize * c.height as usize;
    let full = Picture { planes: [c.planes[0].clone(), c.planes[1].clone(), c.planes[2].clone(), Some(c.planes[3].clone().unwrap_or(vec![0xff; n]))], ..c.clone() };
    let blank = Picture { planes: [Some(vec![0; n]), Some(vec![0; n]), Some(vec![0; n]), Some(vec![0; n])], ..c.clone() };
    let comp = |dst: &Picture, mode: i16| {
        let mut d = dst.clone();
        composite::picture_composite(&mut d, &full, 0, 0, mode, 1.0, None).ok().map(|_| d)
    };
    out.push(("composite: copy onto blank", comp(&blank, 0).map(|d| same_planes(&d, &full))));
    out.push(("composite: difference with itself", comp(&full, 8).map(|d| same_planes(&d, &blank))));
    out.push(("composite: maximum with itself", comp(&full, 3).map(|d| same_planes(&d, &full))));
    out.push(("composite: over blank, alpha", comp(&blank, 1).map(|d| {
        // where the source has alpha the pixel is the source's; elsewhere the blank stays
        (0..n).all(|i| {
            let a = full.planes[3].as_ref().unwrap()[i];
            (0..4).all(|k| d.planes[k].as_ref().unwrap()[i] == if a != 0 { full.planes[k].as_ref().unwrap()[i] } else { 0 })
        })
    })));
    let fade = |t: f32| {
        let mut a = full.clone();
        composite::picture_fade(&mut a, &blank, t).ok().map(|_| a)
    };
    out.push(("fade by 1 keeps, by 0 replaces", fade(1.0).zip(fade(0.0)).map(|(a, b)| same_planes(&a, &full) && same_planes(&b, &blank))));
    out
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: imgfmt-check <directory>");
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    files.sort();
    // Targa is counted in three rows, because the original's reader does not read back all
    // its writer writes (both run in the emulator say so, finding 0014): it takes the low
    // byte of width and height as a signed value, so only sizes whose low bytes are below
    // 0x80 come back, and it does not give back run-length files at all.
    let names = [
        "targa run-length",
        "targa plain, low sizes",
        "targa plain, others",
        "aurora",
        "alias",
        "raw planes",
        "interleaved",
        "flip x twice",
        "flip y twice",
        "four quarter turns",
        "two half turns",
    ];
    let expected_to_differ = [true, false, true, false, false, false, false, false, false, false, false];
    let mut tallies: Vec<Tally> = names.iter().map(|_| Tally::default()).collect();
    let (mut pictures, mut skipped) = (0usize, 0usize);
    let mut op_rows: Vec<(&'static str, Tally)> = Vec::new();
    for path in &files {
        let Ok(data) = std::fs::read(path) else { continue };
        let Ok(f) = Picture::read(&data) else {
            skipped += 1;
            continue;
        };
        let p = f.picture;
        // the formats here need red, green and blue
        if p.planes[..3].iter().any(|x| x.is_none()) {
            skipped += 1;
            continue;
        }
        pictures += 1;
        let n = p.width as usize * p.height as usize;
        let with_alpha = |q: &Picture| Picture { planes: [q.planes[0].clone(), q.planes[1].clone(), q.planes[2].clone(), Some(q.planes[3].clone().unwrap_or(vec![0xff; n]))], ..q.clone() };
        let low = p.width & 0x80 == 0 && p.height & 0x80 == 0;
        for (i, plain) in [(0usize, false), (if low { 1 } else { 2 }, true)] {
            tallies[i].add(targa::write(&p, plain).ok().and_then(|b| targa::read_tga_picture(&b).ok()).map(|t| {
                // the writer always stores four bytes a pixel, alpha 0xff where there is none,
                // and says in the descriptor whether that fourth byte is an alpha plane
                same_planes(&t.picture, &p)
            }));
        }
        tallies[3].add(aurora::write_aurora(&p).ok().and_then(|b| aurora::read_aurora(&b).ok()).map(|q| same_planes(&q, &with_alpha(&p))));
        tallies[4].add(alias::write_alias(&p).ok().and_then(|b| alias::read_alias(&b).ok()).map(|q| same_planes(&q, &rgb_only(&p))));
        tallies[5].add(raw::write_raw(&p).ok().and_then(|f| {
            let src = raw::RawSource::Files([f[0].as_deref(), f[1].as_deref(), f[2].as_deref(), f[3].as_deref()]);
            raw::read_raw(p.width, p.height, src).ok()
        }).map(|q| same_planes(&q, &p)));
        tallies[6].add(raw::write_rgb(&p, true).ok().and_then(|b| raw::read_rgb(p.width, p.height, true, None, Some(&b)).ok()).map(|q| same_planes(&q, &with_alpha(&p))));
        let twice = |op: fn(&mut Picture) -> Result<(), effect::Fault>, times: usize| {
            let mut q = p.clone();
            for _ in 0..times {
                op(&mut q).ok()?;
            }
            Some(same_planes(&q, &p))
        };
        tallies[7].add(twice(effect::flip_x, 2));
        tallies[8].add(twice(effect::flip_y, 2));
        tallies[9].add(twice(effect::rotate_90, 4));
        tallies[10].add(twice(effect::rotate_180, 2));
        for (i, (name, r)) in operations(&p, &data).into_iter().enumerate() {
            if op_rows.len() <= i {
                op_rows.push((name, Tally::default()));
            }
            op_rows[i].1.add(r);
        }
    }
    println!("{} .pic files, {} pictures with red, green and blue, {} not used", files.len(), pictures, skipped);
    println!("{:<26}{:>9}{:>11}{:>9}", "round trip", "same", "different", "refused");
    for (name, t) in names.iter().zip(&tallies) {
        println!("{:<26}{:>9}{:>11}{:>9}", name, t.same, t.different, t.refused);
    }
    println!();
    println!("{:<36}{:>9}{:>11}{:>16}", "operation, port against itself", "holds", "does not", "not applicable");
    for (name, t) in &op_rows {
        println!("{:<36}{:>9}{:>11}{:>16}", name, t.same, t.different, t.refused);
    }
    let bad = tallies.iter().zip(expected_to_differ).any(|(t, e)| !e && (t.different != 0 || t.refused != 0))
        || op_rows.iter().any(|(name, t)| t.different != 0 && !name.ends_with("height = rows made"));
    std::process::exit(if bad { 1 } else { 0 });
}
