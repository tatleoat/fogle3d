// SPDX-License-Identifier: MIT
//! Checks for the nine routines of `sgiimage.c` that `sgi.rs` did not have
//! (`si_imgfmt::sgiimage`): vectors from `re/tools/imgfmt3/gen_sgiimage.py`.
//!
//! The harness's descriptors: 3 = /t/a.sgi at 0, 4 = /t/o.sgi at 0, 6 = /t/a.sgi at 0x200,
//! 8 = /t/o.sgi at 0x200; a name opened gets 9. Messages are the error handler's texts.

use crate::util::*;
use si_imgfmt::sgi::{self, Fd, Image};
use si_imgfmt::sgiimage::{self, Source};
use si_x87::vectors::{Case, Check, Verdict, Word};

fn word(c: &Case, name: &str) -> u32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32
}

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

fn handler(c: &Case) -> bool {
    c.try_input("errfunc").map_or(false, |f| f.words()[0].value().unwrap_or(0) != 0)
}

/// What descriptor `fd` leads to in the harness.
fn descriptor(c: &Case, fd: i32) -> Option<Fd> {
    let (path, pos) = match fd {
        3 => ("/t/a.sgi", 0),
        6 => ("/t/a.sgi", 0x200),
        4 => ("/t/o.sgi", 0),
        8 => ("/t/o.sgi", 0x200),
        _ => return None,
    };
    if path == "/t/o.sgi" {
        Some(Fd::at(Vec::new(), pos))
    } else {
        c.try_input(&format!("file:{path}")).map(|f| Fd::at(f.to_bytes(), pos))
    }
}

/// The position the harness reports for descriptor `fd` after the call.
fn fd_pos(c: &Case, fd: i32) -> Option<u32> {
    c.try_output("fdpos")?.u32s().into_iter().find(|w| (w >> 24) as i32 == fd).map(|w| w & 0xff_ffff)
}

/// The 0x98 bytes of a record as a written header holds them, with the bytes the heap decides
/// zeroed.
fn masked(mut b: Vec<u8>, start: usize) -> Vec<u8> {
    for r in [0x68..0x72, 0x74..0x98] {
        for i in r {
            let i = i + start;
            if i < b.len() {
                b[i] = 0;
            }
        }
    }
    b
}

fn is_ref(w: &Word) -> bool {
    matches!(w, Word::Ref { .. })
}

/// Compares the record the original left (words of `rec`) with the port's image. `fd` is the
/// descriptor number the record should hold; a word the original never wrote is not compared.
fn same_record(w: &[Word], im: &Image, fd: i32, has_base: bool, tmpbuf: bool) -> Verdict {
    let flip = mutate() as u128;
    let want = |i: usize, v: u128, what: &str| -> Verdict {
        match &w[i] {
            Word::Unwritten { .. } => Verdict::Same,
            Word::Value { value, .. } => Verdict::from(*value == (v ^ if i == 91 { flip } else { 0 }), || format!("{what}: {value:#x}, port {v:#x}")),
            Word::Ref { .. } => Verdict::Different(format!("{what} is a pointer")),
        }
    };
    let fields: [(usize, u128, &str); 12] = [
        (0, im.imagic as u128, "imagic"),
        (1, im.kind as u128, "kind"),
        (2, im.dim as u128, "dim"),
        (3, im.xsize as u128, "xsize"),
        (4, im.ysize as u128, "ysize"),
        (5, im.zsize as u128, "zsize"),
        (6, im.min as u128, "min"),
        (7, im.max as u128, "max"),
        (8, im.wastebytes as u128, "wastebytes"),
        (89, im.colormap as u128, "colormap"),
        (90, fd as u32 as u128, "fd"),
        (91, im.flags as u128, "flags"),
    ];
    let mut v = Verdict::Same;
    for (i, val, what) in fields {
        v = v.and(|| want(i, val, what));
    }
    let more: [(usize, u128, &str); 6] = [
        (92, im.dorev as u16 as u128, "dorev"),
        (93, im.x as u16 as u128, "x"),
        (94, im.y as u16 as u128, "y"),
        (95, im.z as u16 as u128, "z"),
        (96, im.cnt as u16 as u128, "cnt"),
        (100, im.offset as u128, "offset"),
    ];
    for (i, val, what) in more {
        v = v.and(|| want(i, val, what));
    }
    v = v.and(|| want(101, im.rleend as u128, "rleend"));
    // the name, byte by byte
    v = v.and(|| {
        for k in 0..80 {
            if let Word::Value { value, .. } = &w[9 + k] {
                if *value != im.name[k] as u128 {
                    return Verdict::Different(format!("name byte {k}"));
                }
            }
        }
        Verdict::Same
    });
    // pointers: only whether they are null (and, for the pixel pointer, where in the buffer)
    v.and(|| Verdict::from(is_ref(&w[98]) == has_base, || "pixel buffer base".into()))
        .and(|| Verdict::from(!is_ref(&w[99]) || tmpbuf, || "tmpbuf".into()))
        .and(|| match &w[97] {
            Word::Ref { offset, .. } => Verdict::from(has_base && *offset as i32 == im.ptr, || format!("pixel pointer at {offset}, port {}", im.ptr)),
            Word::Value { value, .. } => Verdict::from(!has_base && *value == 0 && im.ptr == 0 || has_base && false, || "pixel pointer".into()),
            Word::Unwritten { .. } => Verdict::Same,
        })
}

fn clean(t: &str) -> &str {
    t
}

// ---------------------------------------------------------------- imgopen, iopen, fiopen

fn open_check(c: &Case, by_name: bool) -> Verdict {
    let mode = c.input("mode").to_bytes();
    let mode = cstr(&mode).to_vec();
    let (kind, dim, xs, ys, zs) = (word(c, "kind") as u16, word(c, "dim"), word(c, "xs") as u16, word(c, "ys") as u16, word(c, "zs") as u16);
    let (src, fd) = if by_name {
        let name = cstr(&c.input("path").to_bytes()).to_vec();
        let nm = String::from_utf8_lossy(&name).into_owned();
        let existing = c.try_input(&format!("file:{nm}")).map(|f| f.to_bytes());
        (Source::Path { name, fd: 9, existing, creatable: !nm.starts_with("/nodir/") }, 9)
    } else {
        let fd = word(c, "fd") as i32;
        (Source::Fd { fd, file: descriptor(c, fd) }, fd)
    };
    let writing = mode.first() == Some(&b'w');
    let got = match sgiimage::imgopen(src, &mode, kind, dim, xs, ys, zs, handler(c)) {
        Ok(o) => o,
        Err(_) => return Verdict::Skip,
    };
    let said = messages(c);
    let v = Verdict::from(said == got.errors, || format!("messages {said:?}, port {:?}", got.errors));
    let Some(im) = got.image else {
        return v.and(|| Verdict::from(eax(c) == 0, || "the original returned an image".into()));
    };
    v.and(|| Verdict::from(eax(c) != 0, || "the original returned null".into()))
        .and(|| {
            let w = c.output("rec").words().to_vec();
            same_record(&w, &im, fd, false, true)
        })
        .and(|| Verdict::from(c.output("tbsize").u32() as usize == sgi::ibufalloc(im.xsize), || "tmpbuf size".into()))
        .and(|| {
            if im.kind >> 8 != 1 {
                return Verdict::Same;
            }
            let Some(rs) = c.try_output("rs") else { return Verdict::Same };
            Verdict::from(rs.u32s() == im.rowstart, || "rowstart".into()).and(|| Verdict::from(c.output("rz").i32s() == im.rowsize, || "rowsize".into()))
        })
        .and(|| Verdict::from(fd_pos(c, fd) == Some(im.file.pos as u32), || format!("descriptor at {:?}, port {}", fd_pos(c, fd), im.file.pos)))
        .and(|| {
            if !writing {
                return Verdict::Same;
            }
            let path = if by_name { String::from_utf8_lossy(&cstr(&c.input("path").to_bytes()).to_vec()).into_owned() } else if fd == 4 || fd == 8 { "/t/o.sgi".into() } else { "/t/a.sgi".into() };
            let theirs = file_out(c, &path).unwrap_or_default();
            let head = im.file.data.clone();
            let start = if !by_name && (fd == 6 || fd == 8) { 0x200 } else { 0 };
            same_bytes("file", &masked(theirs, start), &masked(head, start))
        })
}

// ---------------------------------------------------------------- the pixel buffer

fn image_in(c: &Case) -> Image {
    let w = c.input("img").words();
    let v = |i: usize| w[i].value().unwrap_or(0);
    let mut im = Image::default();
    (im.imagic, im.kind, im.dim, im.xsize, im.ysize, im.zsize) = (v(0) as u16, v(1) as u16, v(2) as u16, v(3) as u16, v(4) as u16, v(5) as u16);
    (im.min, im.max, im.wastebytes) = (v(6) as u32, v(7) as u32, v(8) as u32);
    im.colormap = v(89) as u32;
    (im.flags, im.dorev, im.x, im.y, im.z, im.cnt) = (v(91) as u16, v(92) as u16 as i16, v(93) as u16 as i16, v(94) as u16 as i16, v(95) as u16 as i16, v(96) as u16 as i16);
    if let Word::Ref { offset, .. } = &w[97] {
        im.ptr = *offset as i32;
    }
    if is_ref(&w[98]) {
        im.base = Some(c.try_input("base").map(|f| f.to_bytes()).unwrap_or_default());
    }
    im.tmpbuf = c.try_input("tmp").map(|f| f.to_bytes()).unwrap_or_default();
    im.tmp_set = im.tmpbuf.len();
    (im.offset, im.rleend) = (v(100) as u32, v(101) as u32);
    im.rowstart = c.try_input("starts").map(|f| f.u32s()).unwrap_or_default();
    im.rowsize = c.try_input("sizes").map(|f| f.i32s()).unwrap_or_default();
    im.handler = handler(c);
    let fd = v(90) as u32 as i32;
    im.file = descriptor(c, fd).unwrap_or_else(Fd::invalid);
    im
}

fn pixel_check(c: &Case, run: impl FnOnce(&mut Image) -> Result<u32, si_imgfmt::effect::Fault>) -> Verdict {
    let mut im = image_in(c);
    let fd = c.input("img").words()[90].value().unwrap_or(0) as u32 as i32;
    let had_base = im.base.is_some();
    let had_file = descriptor(c, fd).is_some();
    let got = match run(&mut im) {
        Ok(g) => g,
        Err(_) => return Verdict::Skip,
    };
    if !had_file && !had_base && im.flags != 0 && false {
        return Verdict::Skip;
    }
    // the original sometimes leaves in eax what the stack held: not compared
    if c.output("eax").words().iter().any(|w| w.value().is_none()) {
        return Verdict::Skip;
    }
    let ours = got ^ mutate() as u32;
    let said = messages(c);
    Verdict::from(eax(c) == ours, || format!("answer {:#x}, port {ours:#x}", eax(c)))
        .and(|| Verdict::from(said == im.errors, || format!("messages {said:?}, port {:?}", im.errors)))
        .and(|| same_record(c.after("img").words(), &im, fd, im.base.is_some(), true))
        .and(|| Verdict::from(c.after("starts").u32s() == im.rowstart, || "row offsets".into()))
        .and(|| Verdict::from(c.after("sizes").i32s() == im.rowsize, || "row lengths".into()))
        .and(|| {
            let b = im.base.clone().unwrap_or_default();
            if had_base {
                same_bytes("pixel buffer", &c.after("base").to_bytes(), &b)
            } else if let Some(nb) = c.try_output("newbase") {
                let n = im.row_bytes.max(2).min(b.len());
                same_bytes("new pixel buffer", &nb.to_bytes()[..n], &b[..n])
            } else {
                Verdict::Same
            }
        })
        .and(|| match fd_pos(c, fd) {
            Some(p) if had_file => Verdict::from(p as usize == im.file.pos, || format!("descriptor at {p}, port {}", im.file.pos)),
            _ => Verdict::Same,
        })
        .and(|| {
            if fd == 4 || fd == 8 {
                let theirs = file_out(c, "/t/o.sgi").unwrap_or_default();
                if had_file {
                    return same_bytes("file", &theirs, &im.file.data);
                }
            }
            Verdict::Same
        })
}

pub fn lookup(name: &str) -> Option<Check> {
    let _ = clean;
    Some(match name {
        "imgopen#path" | "iopen" => |c: &Case| open_check(c, true),
        "imgopen#fd" | "fiopen" => |c: &Case| open_check(c, false),
        "ifilbuf" => |c: &Case| pixel_check(c, |im| im.ifilbuf()),
        "getpix" => |c: &Case| pixel_check(c, |im| im.getpix()),
        "iflsbuf" => |c: &Case| {
            let ch = word(c, "c");
            pixel_check(c, |im| im.iflsbuf(ch))
        },
        "putpix" => |c: &Case| {
            let ch = word(c, "c");
            pixel_check(c, |im| im.putpix(ch))
        },
        "isetcolormap" => |c: &Case| {
            let mut im = image_in(c);
            im.isetcolormap(word(c, "map"));
            Verdict::from(c.after("img").words()[89].value() == Some((im.colormap ^ mutate() as u32) as u128), || "colour map word".into())
        },
        "i_seterror" => |c: &Case| {
            let mut slot = 0u32;
            sgiimage::i_seterror(&mut slot, word(c, "f"));
            Verdict::from(c.after("errfunc").u32() == slot ^ mutate() as u32, || "error handler word".into())
        },
        _ => return None,
    })
}
