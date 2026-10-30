// SPDX-License-Identifier: MIT
//! Checks for `picture.c`, `geticon.c` and `util.c` (crate `si-picture`).

use crate::util::*;
use si_picture::*;
use si_x87::vectors::{Case, Check, Verdict};

const IN: &str = "/t/a.pic";
const OUT: &str = "/t/o.pic";

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

/// A written picture file with the chunk's spare word cleared, as the generator clears it in
/// the original's output (stale stack there).
fn no_pad(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() >= 104 {
        bytes[102] = 0;
        bytes[103] = 0;
    }
    bytes
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "si_packBit" => |c: &Case| {
            let n = c.input("n").i32() as usize;
            let ours = pack_bit(&c.input("src").to_bytes()[..n]);
            let theirs = c.output("buf").to_bytes();
            Verdict::from(eax(c) as usize == ours.len(), || format!("length {} against {}", eax(c), ours.len()))
                .and(|| same_bytes("packed", &theirs[..ours.len()], &ours))
        },
        "si_unPackBit" => |c: &Case| {
            let ours = un_pack_bit(&c.input("src").to_bytes(), c.input("n").i32() as i64);
            let theirs: Vec<Option<u128>> = c.output("dst").written();
            let written = theirs.iter().take_while(|w| w.is_some()).count();
            let got: Vec<u8> = theirs[..written].iter().map(|w| w.unwrap() as u8).collect();
            same_bytes("unpacked", &got, &ours)
        },
        "SI_pictureRead" => |c: &Case| {
            let words = c.output("pic").words();
            match Picture::read(&file_in(c, IN)) {
                Ok(f) => Verdict::from(eax(c) == 1, || "the original refused".into()).and(|| same_picture(c, words, &f.picture)),
                Err(e) => {
                    let (w, h) = (words[0].value().unwrap_or(0) as u16 as i16, words[1].value().unwrap_or(0) as u16 as i16);
                    Verdict::from(eax(c) == 0, || format!("the port refused: {e:?}"))
                        .and(|| Verdict::from(messages(c) == [e.message(IN, w, h)], || format!("messages {:?}", messages(c))))
                }
            }
        },
        "SI_pictureWrite" => |c: &Case| {
            let p = picture(c, c.input("pic").words());
            let comment = c.input("comment").to_bytes();
            let ours = no_pad(p.write(Some(&comment), false));
            match file_out(c, OUT) {
                Some(theirs) => same_bytes("file", &theirs, &ours),
                None => Verdict::Different("no file written".into()),
            }
        },
        "SI_pictureReadRGBAScale" => |c: &Case| packed_reader(c, 0),
        "SI_pictureReadBWScale" => |c: &Case| packed_reader(c, 1),
        "SI_pictureReadScaledRGBAScale" => |c: &Case| packed_reader(c, 2),
        "SI_pictureRGBAWrite" => |c: &Case| {
            let words = c.input("p").words();
            let n = words[0].value().unwrap() as usize * words[1].value().unwrap() as usize;
            let p = rgba(c, words, n, 4).unwrap();
            let ours = no_pad(rgba_write(&p, Some(&c.input("comment").to_bytes()), false).unwrap());
            match file_out(c, OUT) {
                Some(theirs) => same_bytes("file", &theirs, &ours),
                None => Verdict::Different("no file written".into()),
            }
        },
        "SI_pictureInfo" => |c: &Case| {
            let theirs = text(c.output("out"));
            match picture_info(&file_in(c, IN), IN, "a.pic") {
                Ok(t) => Verdict::from(eax(c) == 1 && theirs == t, || format!("original {theirs:?} port {t:?}")),
                Err((t, m)) => Verdict::from(eax(c) == 0 && theirs == t, || format!("original {theirs:?} port {t:?}"))
                    .and(|| Verdict::from(messages(c) == if m.is_empty() { vec![] } else { vec![m.clone()] }, || format!("messages {:?} port {m:?}", messages(c)))),
            }
        },
        "SI_pictureGetInfo" => |c: &Case| {
            let theirs = (c.output("w").i32(), c.output("h").i32(), c.output("ratio").u32(), c.output("alpha").i16());
            match picture_get_info(&file_in(c, IN)) {
                Ok(i) => Verdict::from(eax(c) == 1 && theirs == (i.width, i.height, i.ratio.to_bits(), i.has_alpha), || format!("original {theirs:?} port {i:?}")),
                Err(e) => Verdict::from(eax(c) == 0 && theirs == (0, 0, 0, 0), || format!("original {theirs:?} port {e:?}"))
                    .and(|| Verdict::from(messages(c) == e.message("a.pic").into_iter().collect::<Vec<_>>(), || format!("messages {:?}", messages(c)))),
            }
        },
        "SI_pictureGetDimensions" => |c: &Case| match picture_get_dimensions(&file_in(c, IN)) {
            Ok((w, h)) => Verdict::from(eax(c) == 0 && (c.output("w").i16(), c.output("h").i16()) == (w, h), || format!("port {w} x {h}")),
            Err(e) => Verdict::from(eax(c) == 4, || format!("port refused: {e:?}")),
        },
        "SI_constructPictureComment" => |c: &Case| {
            let mut ours = cstr(&c.input("comment").to_bytes()).to_vec();
            match construct_picture_comment(&file_in(c, IN), c.input("cmd").i32(), &mut ours) {
                Ok(()) => same_bytes("comment", cstr(&c.after("comment").to_bytes()), &ours),
                Err(e) => Verdict::Different(format!("the port stops the program: {e:?}")),
            }
        },
        "SI_fdpictureOpenFile" => |c: &Case| {
            let data = c.input("f").to_bytes();
            let version = c.output("version").written()[0];
            let comment = c.output("comment").written();
            let theirs_comment: Vec<u8> = comment.iter().take_while(|w| w.is_some_and(|v| v != 0)).map(|w| w.unwrap() as u8).collect();
            match OpenPicture::open_fd(&data) {
                Ok(f) => {
                    let packets = c.outputs.iter().filter(|f| f.name.starts_with("heap")).count();
                    Verdict::from(eax(c) == 1, || "the original refused".into())
                        .and(|| same_head(c.output("pic").words(), f.picture.width, f.picture.height, f.picture.ratio, f.picture.fields, f.picture.pad))
                        .and(|| Verdict::from(version == Some(f.version.to_bits() as u128) && theirs_comment == f.comment, || "version or comment".into()))
                        .and(|| Verdict::from(packets == f.packets.len(), || format!("{packets} packets, port {}", f.packets.len())))
                }
                Err(e) => Verdict::from(eax(c) == 0, || format!("the port refused: {e:?}"))
                    .and(|| Verdict::from(messages(c) == e.message().map(String::from).into_iter().collect::<Vec<_>>(), || format!("messages {:?}", messages(c))))
                    .and(|| match &e {
                        FdOpenError::NotSoftimage { version: v, comment: k } => {
                            Verdict::from(version == Some(v.to_bits() as u128) && &theirs_comment == k, || "version or comment of a refused file".into())
                        }
                        _ => Verdict::Same,
                    }),
            }
        },
        "SI_fdpictureWrite" => |c: &Case| {
            let p = picture(c, c.input("pic").words());
            same_bytes("stream", &c.output("f").to_bytes(), &no_pad(fd_write(&p, Some(&c.input("comment").to_bytes()))))
        },
        "SI_picRgbaBorder" => |c: &Case| {
            let (mut p, n) = packed_in(c, 4);
            border(&mut p);
            same_bytes("pixels", &c.after("px").to_bytes()[..4 * n], &p.data)
        },
        "SI_picRgbaSwapBits" => |c: &Case| {
            let (mut p, _) = packed_in(c, 4);
            swap_bits_rgba(&mut p);
            same_rgba(c, c.output("p").words(), &p)
        },
        "picAlphaRemove" => |c: &Case| {
            let (mut p, _) = packed_in(c, 4);
            alpha_remove(&mut p);
            same_rgba(c, c.output("p").words(), &p)
        },
        "SI_picRgbSwapBits" => |c: &Case| {
            let (mut p, _) = packed_in(c, 3);
            swap_bits_rgb(&mut p);
            same_rgba(c, c.output("p").words(), &p)
        },
        "SI_cropPic" => |c: &Case| {
            let r = c.input("rect").i16s();
            returned(c, crop(&packed_in(c, 4).0, Irect { x0: r[0], x1: r[1], y0: r[2], y1: r[3] }))
        },
        "SI_resizePic" => |c: &Case| returned(c, resize(&packed_in(c, 4).0, c.input("w").i32(), c.input("h").i32())),
        "SI_picEnlarge" => |c: &Case| {
            let m = c.input("map").to_bytes();
            let blend = f32::from_le_bytes([m[0x58], m[0x59], m[0x5a], m[0x5b]]);
            returned(c, enlarge(&packed_in(c, 4).0, blend, i16::from_le_bytes([m[0xfc], m[0xfd]])))
        },
        "SI_cropPicCI" => |c: &Case| {
            let r = c.input("rect").i16s();
            returned_ci(c, crop_ci(&ci_in(c), Irect { x0: r[0], x1: r[1], y0: r[2], y1: r[3] }), false)
        },
        "SI_resizePicCI" => |c: &Case| returned_ci(c, resize_ci(&ci_in(c), c.input("w").i32(), c.input("h").i32()), false),
        "SI_picEnlargeCI" => |c: &Case| {
            let m = c.input("map").to_bytes();
            returned_ci(c, enlarge_ci(&ci_in(c), f32::from_le_bytes([m[0x58], m[0x59], m[0x5a], m[0x5b]])), true)
        },
        "SI_pictReadAsIcon" => |c: &Case| match pict_read_as_icon(&file_in(c, IN)) {
            Ok(p) => Verdict::from(eax(c) != 0, || "the original refused".into()).and(|| same_returned_picture(c, &p)),
            Err(e) => Verdict::from(eax(c) == 0, || format!("the port refused: {e:?}")),
        },
        "pictReadAsIconRGBA" => |c: &Case| match pict_read_as_icon_rgba(&file_in(c, IN)) {
            Ok(p) => Verdict::from(eax(c) != 0, || "the original refused".into()).and(|| same_returned_rgba(c, &p)),
            Err(e) => Verdict::from(eax(c) == 0, || format!("the port refused: {e:?}")),
        },
        "SI_iconLoadFromPicRGBA" => |c: &Case| {
            let words = c.input("p").words();
            let n = words[0].value().unwrap() as usize * words[1].value().unwrap() as usize;
            same_returned_rgba(c, &icon_from_rgba(&rgba(c, words, n, 4).unwrap()))
        },
        "fileIconRgbaReadF" => |c: &Case| returned(c, read_icon_rgba(&c.input("f").to_bytes())),
        "SI_fileIconRead" => |c: &Case| match read_icon(&file_in(c, IN)) {
            Some((p, _)) => Verdict::from(eax(c) != 0, || "the original found no icon".into()).and(|| same_returned_picture(c, &p)),
            None => Verdict::from(eax(c) == 0, || "the port found no icon".into()),
        },
        "SI_fileIconWrite" => |c: &Case| {
            let mut file = file_in(c, IN);
            write_icon(&mut file, &picture(c, c.input("pic").words()));
            same_bytes("file", &file_out(c, IN).unwrap_or_else(|| file_in(c, IN)), &file)
        },
        "SI_addIconToPic" => |c: &Case| {
            let mut file = file_in(c, IN);
            let r = add_icon_to_pic(&mut file, IN);
            Verdict::from((eax(c) & 0xffff == 1) == r.is_ok(), || format!("port {r:?}"))
                .and(|| Verdict::from(messages(c).last().cloned() == r.clone().err(), || format!("messages {:?}", messages(c))))
                .and(|| same_bytes("file", &file_out(c, IN).unwrap_or_else(|| file_in(c, IN)), &file))
        },
        "fileIconRgbaRead" => |c: &Case| {
            let mut file = file_in(c, IN);
            if icon_offset(&file).is_none() {
                // The original adds an icon through one handle and opens the file again to read
                // it. The harness hands the second open the case's first bytes, without the
                // icon, so what the original would read back cannot be seen here.
                return Verdict::Skip;
            }
            let r = file_icon_rgba_read(&mut file, IN);
            returned(c, r).and(|| same_bytes("file", &file_out(c, IN).unwrap_or_else(|| file_in(c, IN)), &file))
        },
        _ => return None,
    })
}

/// The three packed readers. With scaling the original calls `SI_scalePictureX` (stubbed
/// and recorded by the generator) exactly when the port calls its rescaler.
fn packed_reader(c: &Case, which: u8) -> Verdict {
    let data = file_in(c, IN);
    let calls = std::cell::Cell::new(0usize);
    let rescale = |_: &mut Picture, _: f32| calls.set(calls.get() + 1);
    let scale: Option<ScaleX> = (c.input("scale").i16() != 0).then_some(&rescale);
    let r = match which {
        0 => read_rgba_scale(&data, scale),
        1 => read_bw_scale(&data, scale),
        _ => read_scaled_rgba_scale(&data, scale),
    };
    let theirs = c.calls.iter().filter(|k| k.name == "SI_scalePictureX").count();
    match r {
        Ok(p) => Verdict::from(eax(c) == 1, || "the original refused".into())
            .and(|| Verdict::from(theirs == calls.get(), || format!("{theirs} calls of the rescaler, port {}", calls.get())))
            .and(|| same_head(c.output("p").words(), p.width, p.height, p.ratio, p.fields, p.pad))
            .and(|| match c.try_output("data") {
                Some(d) => same_written("pixels", d, &wide(&p.data)),
                None => Verdict::from(p.data.is_empty(), || "the original has no pixels".into()),
            }),
        Err(e) => {
            let (w, h) = (i16::from_be_bytes([data.get(92).copied().unwrap_or(0), data.get(93).copied().unwrap_or(0)]), i16::from_be_bytes([data.get(94).copied().unwrap_or(0), data.get(95).copied().unwrap_or(0)]));
            Verdict::from(eax(c) == 0, || format!("the port refused: {e:?}"))
                .and(|| Verdict::from(messages(c) == Some(e.message(IN, w, h)).filter(|m| !m.is_empty()).into_iter().collect::<Vec<_>>(), || format!("messages {:?}", messages(c))))
        }
    }
}

/// The packed picture a routine was handed (`p` with its buffer `px`), `per` bytes a pixel.
fn packed_in(c: &Case, per: usize) -> (PictureRgba, usize) {
    let words = c.input("p").words();
    let n = words[0].value().unwrap() as usize * words[1].value().unwrap() as usize;
    (rgba(c, words, n, per).unwrap(), n)
}

fn returned(c: &Case, ours: Option<PictureRgba>) -> Verdict {
    match ours {
        Some(p) => Verdict::from(eax(c) != 0, || "the original returned nothing".into()).and(|| same_returned_rgba(c, &p)),
        None => Verdict::from(eax(c) == 0, || "the port returned nothing".into()),
    }
}

fn ci_in(c: &Case) -> PictureCi {
    let w = c.input("p").words();
    let (width, height) = (w[0].value().unwrap() as u16, w[1].value().unwrap() as u16);
    PictureCi { width, height, size: w[2].value().unwrap() as u32, data: c.input("ix").u16s()[..width as usize * height as usize].to_vec() }
}

fn returned_ci(c: &Case, ours: Option<PictureCi>, with_size: bool) -> Verdict {
    let Some(p) = ours else { return Verdict::from(eax(c) == 0, || "the port returned nothing".into()) };
    if eax(c) == 0 {
        return Verdict::Different("the original returned nothing".into());
    }
    let rec = c.output("rec").written();
    let head = (rec[0], rec[1]) == (Some(p.width as u128), Some(p.height as u128)) && (!with_size || rec[2] == Some(p.size as u128));
    Verdict::from(head, || format!("record {:?}, port {} x {} size {}", &rec[..3], p.width, p.height, p.size)).and(|| match c.try_output("data") {
        Some(d) => same_written("indices", d, &p.data.iter().map(|&v| v as u128).collect::<Vec<_>>()),
        None => Verdict::from(p.data.is_empty(), || "the original has no indices".into()),
    })
}
