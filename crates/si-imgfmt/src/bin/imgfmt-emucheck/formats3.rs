// SPDX-License-Identifier: MIT
//! Checks for the formats of the third pass: YUV (`si_imgfmt::yuv`) and SGI
//! (`si_imgfmt::sgi`, with the image library's routines it stands on).

use crate::util::*;
use si_imgfmt::sgi::{self, Image, OpenError, Written};
use si_imgfmt::yuv;
use si_picture::Picture;
use si_x87::vectors::{Case, Check, Verdict, Word};

fn word(c: &Case, name: &str) -> i32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32 as i32
}

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

/// The value of `ABEKAS` the generator set for the case: its note in the buffer `env`.
fn abekas(c: &Case) -> Option<Vec<u8>> {
    let e = c.input("env").to_bytes();
    (e[0] == b'=').then(|| cstr(&e[1..]).to_vec())
}

/// A picture record of the case whose planes hold `cap` bytes each: all of them are handed
/// to the port, whatever the record's size says (a routine may read by another row length).
fn picture_whole(c: &Case, name: &str) -> Picture {
    let w = c.input(name).words();
    let mut p = picture_in(c, w);
    for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
        if matches!(w[5 + i], Word::Ref { .. }) {
            p.planes[i] = Some(c.input(n).to_bytes());
        }
    }
    p
}

fn messages_to_handler(c: &Case) -> Vec<String> {
    c.outputs.iter().filter(|f| f.name.starts_with("str")).map(text).collect()
}

fn same_planes(c: &Case, p: &Picture, count: usize) -> Verdict {
    let mut v = Verdict::Same;
    for i in 0..count {
        v = v.and(|| match (c.try_output(&format!("p{i}")), &p.planes[i]) {
            (Some(d), Some(ours)) => same_written(&format!("plane {i}"), d, &wide(ours)),
            (None, None) => Verdict::Same,
            (None, Some(ours)) => Verdict::from(ours.is_empty(), || format!("plane {i} missing in the original")),
            (Some(_), None) => Verdict::Different(format!("plane {i} missing in the port")),
        });
    }
    v
}

fn image_in(c: &Case) -> Image {
    let w = c.input("img").words();
    let v = |i: usize| w[i].value().unwrap_or(0);
    let mut im = Image::default();
    (im.imagic, im.kind, im.dim, im.xsize, im.ysize, im.zsize) = (v(0) as u16, v(1) as u16, v(2) as u16, v(3) as u16, v(4) as u16, v(5) as u16);
    (im.min, im.max, im.wastebytes) = (v(6) as u32, v(7) as u32, v(8) as u32);
    // 80 name bytes follow at 9..89
    im.colormap = v(89) as u32;
    (im.flags, im.dorev, im.x, im.y, im.z) = (v(91) as u16, v(92) as u16 as i16, v(93) as u16 as i16, v(94) as u16 as i16, v(95) as u16 as i16);
    (im.offset, im.rleend) = (v(100) as u32, v(101) as u32);
    im.rowstart = c.input("starts").u32s();
    im.rowsize = c.input("sizes").i32s();
    im
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureSoftToYuv" => |c: &Case| {
            let p = picture_whole(c, "pic");
            let lines = word(c, "lines");
            // the frame handed over is the test buffer moved back: rebuild the whole frame
            let Word::Ref { offset, .. } = &c.input("buf").words()[0] else { return Verdict::Skip };
            let back = (*offset as i32).unsigned_abs() as usize;
            let before = c.input("frame").to_bytes();
            let mut frame = vec![0u8; back + before.len()];
            frame[back..].copy_from_slice(&before);
            match yuv::soft_to_yuv(&p, &mut frame, lines, word(c, "key") != 0) {
                Ok(done) => Verdict::from(eax(c) == done as u32, || format!("returned {}, port {done}", eax(c))).and(|| same_bytes("frame", &c.after("frame").to_bytes(), &frame[back..])),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_pictureYuvToSoft" => |c: &Case| {
            let height = c.input("pic").words()[1].value().unwrap_or(0) as u16 as i16;
            match yuv::yuv_to_soft(height, &c.input("words").u32s()) {
                Ok(Some(p)) => Verdict::from(eax(c) == 1, || "the original failed".into())
                    .and(|| same_head(&c.after("pic").words()[..4], p.width, p.height, p.ratio, p.fields, 0))
                    .and(|| Verdict::from(c.after("pic").words()[8].value() == Some(0), || "an alpha plane".into()))
                    .and(|| same_planes(c, &p, 3)),
                Ok(None) => Verdict::from(eax(c) == 0, || "the original succeeded".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_pictureWriteYuv" => |c: &Case| match yuv::write_yuv(&picture_whole(c, "pic"), word(c, "key") != 0, abekas(c).as_deref()) {
            Ok(file) => Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_bytes("file", &file_out(c, "/t/o.yuv").unwrap_or_default(), &file.unwrap_or_default())),
            Err(_) => Verdict::Skip,
        },
        "SI_pictureReadYuv" => |c: &Case| {
            let Some(file) = c.try_input("file:/t/a.yuv").map(|f| f.to_bytes()) else {
                return Verdict::from(eax(c) == 0, || "read without a file".into());
            };
            match yuv::read_yuv(&file, abekas(c).as_deref()) {
                Ok((p, pairs)) => {
                    if pairs < p.height as usize * 360 {
                        return Verdict::Skip; // the original converts memory nobody set
                    }
                    Verdict::from(eax(c) == 1, || "the original failed".into())
                        .and(|| same_head(&c.output("pic").words()[..4], p.width, p.height, p.ratio, p.fields, 0))
                        .and(|| same_planes(c, &p, 3))
                }
                Err(_) => Verdict::Skip,
            }
        },
        "SI_pictureReadSgi" => |c: &Case| {
            let name = sgi::sgi_file_name(&c.input("name").to_bytes());
            let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
            let said = messages_to_handler(c);
            match sgi::picture_read_sgi(file.as_deref()) {
                Ok(Ok((p, errors))) => Verdict::from(eax(c) == 1, || "the original failed".into())
                    .and(|| same_head(&c.output("pic").words()[..4], p.width, p.height, p.ratio, p.fields, 0))
                    .and(|| Verdict::from(matches!(c.output("pic").words()[8], Word::Ref { .. }) == p.planes[3].is_some(), || "alpha plane".into()))
                    .and(|| same_planes(c, &p, 4))
                    .and(|| Verdict::from(said == errors, || format!("messages {said:?}, port {errors:?}"))),
                Ok(Err(e)) => Verdict::from(eax(c) == 0, || "the original succeeded".into()).and(|| match e {
                    OpenError::Status(t) => Verdict::from(said == [format!("soft: {t}")], || format!("status {said:?}")),
                    OpenError::NotSgi => Verdict::from(said.is_empty(), || format!("status {said:?}")),
                    OpenError::CannotOpen => Verdict::from(c.try_output("stdout").is_some(), || "nothing on standard output".into()),
                }),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_pictureWriteSgi" => |c: &Case| {
            let name = sgi::sgi_file_name(&c.input("name").to_bytes());
            let theirs = file_out(c, &String::from_utf8_lossy(&name)).unwrap_or_default();
            match sgi::picture_write_sgi(&picture_whole(c, "pic")) {
                Ok(Written::File(b)) => Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_bytes("file", &theirs, &b)),
                Ok(Written::HeaderOnly(b)) => Verdict::from(eax(c) == 0, || "the original succeeded".into())
                    .and(|| same_bytes("file", &theirs, &b))
                    .and(|| Verdict::from(messages_to_handler(c) == ["soft: error on table alloc"], || format!("status {:?}", messages_to_handler(c)))),
                Err(_) => Verdict::Skip,
            }
        },
        "img_rle_compact" => |c: &Case| {
            let (ibpp, obpp, n) = (word(c, "ibpp"), word(c, "obpp"), word(c, "n"));
            match sgi::img_rle_compact(&c.input("in").to_bytes(), ibpp, obpp, n) {
                Ok(Ok(packed)) => {
                    let theirs = c.output("out").written();
                    let same = packed.iter().enumerate().all(|(i, &b)| theirs.get(i) == Some(&Some((b ^ (mutate() && i == 0) as u8) as u128))) && theirs[packed.len()..].iter().all(|w| w.is_none());
                    Verdict::from(eax(c) as usize == packed.len() / obpp as usize, || format!("{} units, port {}", eax(c), packed.len() / obpp as usize)).and(|| Verdict::from(same, || "packed row".into()))
                }
                Ok(Err(text)) => Verdict::from(eax(c) == 0 && messages_to_handler(c) == [text.clone()], || format!("port says {text:?}, original {:?}", messages_to_handler(c))),
                Err(_) => Verdict::Skip,
            }
        },
        "img_rle_expand" => |c: &Case| {
            let (ibpp, obpp) = (word(c, "ibpp"), word(c, "obpp"));
            let mut out = vec![0u8; 1400];
            match sgi::img_rle_expand(&c.input("in").to_bytes(), ibpp, &mut out, obpp) {
                Ok(Ok(n)) => {
                    let theirs = c.output("out").written();
                    let same = (0..1400).all(|i| if i < n { theirs[i] == Some((out[i] ^ (mutate() && i == 0) as u8) as u128) } else { theirs[i].is_none() });
                    Verdict::from(same, || format!("expanded row of {n} bytes"))
                }
                Ok(Err(text)) => Verdict::from(messages_to_handler(c) == [text.clone()], || format!("port says {text:?}, original {:?}", messages_to_handler(c))),
                Err(_) => Verdict::Skip,
            }
        },
        "cvtshorts" => |c: &Case| {
            let mut b = c.input("buf").to_bytes();
            match sgi::cvtshorts(&mut b, word(c, "n")) {
                Ok(()) => same_bytes("buffer", &c.after("buf").to_bytes(), &b),
                Err(_) => Verdict::Skip,
            }
        },
        "cvtlongs" => |c: &Case| {
            let b = c.input("buf").to_bytes();
            let mut w: Vec<u32> = b.chunks_exact(4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect();
            match sgi::cvtlongs(&mut w, word(c, "n")) {
                Ok(()) => same_bytes("buffer", &c.after("buf").to_bytes(), &w.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()),
                Err(_) => Verdict::Skip,
            }
        },
        "cvtimage" => |c: &Case| {
            // the header's fields are bytes 0 to 0x18 and 0x68 to 0x6c: each word reversed
            let mut b = c.input("img").to_bytes();
            for k in (0..12).step_by(2) {
                b.swap(k, k + 1);
            }
            for k in [0xc, 0x10, 0x14, 0x68] {
                b[k..k + 4].reverse();
            }
            let mut im = Image::default();
            let before = c.input("img").to_bytes();
            let le16 = |k: usize| u16::from_le_bytes([before[k], before[k + 1]]);
            let le32 = |k: usize| u32::from_le_bytes([before[k], before[k + 1], before[k + 2], before[k + 3]]);
            (im.imagic, im.kind, im.dim, im.xsize, im.ysize, im.zsize) = (le16(0), le16(2), le16(4), le16(6), le16(8), le16(10));
            (im.min, im.max, im.wastebytes, im.colormap) = (le32(0xc), le32(0x10), le32(0x14), le32(0x68));
            im.cvtimage();
            let after = c.after("img").to_bytes();
            let a16 = |k: usize| u16::from_le_bytes([after[k], after[k + 1]]);
            let a32 = |k: usize| u32::from_le_bytes([after[k], after[k + 1], after[k + 2], after[k + 3]]);
            Verdict::from(
                (im.imagic, im.kind, im.dim, im.xsize, im.ysize, im.zsize) == (a16(0), a16(2), a16(4), a16(6), a16(8), a16(10)) && (im.min, im.max, im.wastebytes, im.colormap) == (a32(0xc), a32(0x10), a32(0x14), a32(0x68)),
                || "header fields".into(),
            )
            .and(|| same_bytes("record", &after, &b))
        },
        // the three "swaps" that change nothing: the check is that memory is as it was, and
        // that the value returned is the reversed one
        "sgi_swapImageHeader" => |c: &Case| same_bytes("record", &c.after("img").to_bytes(), &c.input("img").to_bytes()),
        "sgi_byteSwap4" => |c: &Case| {
            let v = c.input("p").u32();
            Verdict::from(eax(c) == sgi::sgi_byte_swap4(v) ^ mutate() as u32 && c.after("p").u32() == v, || "swap of four bytes".into())
        },
        "sgi_byteSwap2" => |c: &Case| {
            let v = c.input("p").words()[0].value().unwrap() as u16;
            Verdict::from(eax(c) == (sgi::sgi_byte_swap2(v) ^ mutate() as u16) as u32 && c.after("p").words()[0].value() == Some(v as u128), || "swap of two bytes".into())
        },
        "isetname" => |c: &Case| {
            let mut im = Image::default();
            im.isetname(&c.input("name").to_bytes());
            let mut b = c.input("img").to_bytes();
            b[0x18..0x68].copy_from_slice(&im.name);
            same_bytes("record", &c.after("img").to_bytes(), &b)
        },
        "ibufalloc" => |c: &Case| {
            let b = c.input("img").to_bytes();
            let ours = sgi::ibufalloc(u16::from_le_bytes([b[6], b[7]]));
            Verdict::from(c.output("size").u32() as usize == ours ^ mutate() as usize, || format!("{} bytes, port {ours}", c.output("size").u32()))
        },
        "img_badrow" => |c: &Case| {
            let ours = image_in(c).img_badrow(c.input("y").u32(), c.input("z").u32());
            Verdict::from((eax(c) != 0) == (ours ^ mutate()), || format!("port {ours}"))
        },
        "img_getrowsize" => |c: &Case| match image_in(c).img_getrowsize() {
            Ok(n) => Verdict::from(eax(c) as i32 == n ^ mutate() as i32, || format!("{}, port {n}", eax(c) as i32)),
            Err(_) => Verdict::Skip,
        },
        "img_setrowsize" => |c: &Case| {
            let mut im = image_in(c);
            match im.img_setrowsize(word(c, "cnt"), word(c, "y"), word(c, "z")) {
                Ok(()) => {
                    let w = c.after("img").words();
                    let flip = mutate() as u32;
                    Verdict::from(w[8].value() == Some(im.wastebytes as u128) && w[101].value() == Some((im.rleend ^ flip) as u128), || "waste and end of data".into())
                        .and(|| Verdict::from(c.after("starts").u32s() == im.rowstart, || "row offsets".into()))
                        .and(|| Verdict::from(c.after("sizes").i32s() == im.rowsize, || "row lengths".into()))
                }
                Err(_) => Verdict::Skip,
            }
        },
        _ => return None,
    })
}
