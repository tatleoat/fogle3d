// SPDX-License-Identifier: MIT
//! Checks for the image operations (crate `si-imgfmt`).

use crate::util::*;
use si_imgfmt::{effect, tile};
use si_picture::Picture;
use si_x87::vectors::{Case, Check, Verdict};

/// An operation in place on `pic`: the record and planes afterwards against ours. Where the
/// port stops because the original would use memory that is not its own, nothing is compared.
fn in_place(c: &Case, op: fn(&mut Picture) -> Result<(), effect::Fault>) -> Verdict {
    let mut p = picture_in(c, c.input("pic").words());
    match op(&mut p) {
        Ok(()) => same_picture_after(c, c.after("pic").words(), &p),
        Err(_) => Verdict::Skip,
    }
}

/// As `same_picture`, for a record whose planes may be the caller's buffers or new ones.
pub fn same_picture_after(c: &Case, words: &[si_x87::vectors::Word], ours: &Picture) -> Verdict {
    let mut v = same_head(words, ours.width, ours.height, ours.ratio, ours.fields, ours.pad);
    let n = (ours.width.max(0) as usize) * (ours.height.max(0) as usize);
    for i in 0..4 {
        v = v.and(|| match (&ours.planes[i], target(c, &words[5 + i])) {
            (None, None) => Verdict::Same,
            (Some(a), Some((f, off))) => {
                let w = f.written();
                if mutate() && n.min(a.len()) > 0 && w.get(off).copied().flatten().is_some() {
                    return Verdict::Different(format!("plane {i}: mutated"));
                }
                match (0..n.min(a.len())).find(|&k| w.get(off + k).copied().flatten().is_some_and(|t| t != a[k] as u128)) {
                    Some(k) => Verdict::Different(format!("plane {i} byte {k}: original {:#x}, port {:#x}", w[off + k].unwrap(), a[k])),
                    None => Verdict::Same,
                }
            }
            (Some(a), None) => Verdict::from(n == 0 || a.is_empty(), || format!("plane {i} missing in the original")),
            (None, Some(_)) => Verdict::from(n == 0, || format!("plane {i} missing in the port")),
        });
    }
    v
}

fn two(c: &Case, op: &dyn Fn(&mut Picture, &Picture) -> Result<(), effect::Fault>) -> Verdict {
    let src = picture_in(c, c.input("src").words());
    let mut dst = picture_in(c, c.input("dst").words());
    // the destination's planes as the caller allocated them: 64 bytes each
    for (i, name) in ["dr", "dg", "db", "da"].iter().enumerate() {
        if dst.planes[i].is_some() {
            dst.planes[i] = Some(c.input(name).to_bytes());
        }
    }
    match op(&mut dst, &src) {
        Ok(()) => same_picture_after(c, c.after("dst").words(), &dst),
        Err(_) => Verdict::Skip,
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureFlipX" => |c: &Case| in_place(c, effect::flip_x),
        "SI_pictureFlipY" => |c: &Case| in_place(c, effect::flip_y),
        "SI_pictureRotate90" => |c: &Case| in_place(c, effect::rotate_90),
        "SI_pictureRotate180" => |c: &Case| in_place(c, effect::rotate_180),
        "SI_pictureRotate270" => |c: &Case| in_place(c, effect::rotate_270),
        "SI_pictureNegate" => |c: &Case| two(c, &|d, s| effect::negate(d, s)),
        "SI_channelExtract" => |c: &Case| {
            let ch = c.input("channel").i32();
            two(c, &|d, s| effect::channel_extract(d, s, ch))
        },
        "SI_intensityModifyPicture" => |c: &Case| {
            let f = ["fr", "fg", "fb", "fa"].map(|n| c.input(n).f32());
            let p = ["pr", "pg", "pb", "pa"].map(|n| c.input(n).i32());
            two(c, &|d, s| effect::intensity_modify(d, s, f, p))
        },
        "pictureSaturate" => |c: &Case| {
            let p = ["pr", "pg", "pb", "pa"].map(|n| c.input(n).i32());
            two(c, &|d, s| effect::saturate(d, s, p))
        },
        "SI_pictureRotate" => |c: &Case| {
            // the fourth parameter is not used for the header: its comment is fixed
            let src = match Picture::read(&file_in(c, "/t/a.pic")) {
                Ok(f) => f.picture,
                Err(_) => return Verdict::Skip,
            };
            let clear = |mut b: Vec<u8>| {
                if b.len() >= 104 {
                    b[102] = 0;
                    b[103] = 0;
                }
                b
            };
            let angle = c.input("angle").i32();
            let Some(theirs) = file_out(c, "/t/o.pic") else { return Verdict::Different("the original wrote no file".into()) };
            match effect::rotate_file(&src, angle, c.input("mem").u32()) {
                Ok(w) => Verdict::from(c.output("eax").u32() == 1, || "the original failed".into())
                    .and(|| same_bytes("file", &theirs, &clear(w.file(effect::ROTATE_COMMENT, false)))),
                Err(e @ effect::RotateError::InvalidAngle(_)) => {
                    let head = effect::rotated_header(&src, angle);
                    let has = [0, 1, 2, 3].map(|i| src.planes[i].is_some());
                    let ours = clear(si_picture::write_header(&head, has, Some(effect::ROTATE_COMMENT), false));
                    Verdict::from(c.output("eax").u32() == 0, || "the original succeeded".into())
                        .and(|| same_bytes("header of a refused angle", &theirs, &ours))
                        .and(|| Verdict::from(messages(c) == [e.message("a.pic")], || format!("messages {:?}, port {:?}", messages(c), e.message("a.pic"))))
                }
                Err(_) => Verdict::Skip,
            }
        },
        "tilePic__FP14SI_PictureRGBAN30ii" => |c: &Case| {
            let get = |n: &str, px: &str| {
                let w = c.input(n).words();
                let pixels = w[0].value().unwrap() as usize * w[1].value().unwrap() as usize;
                si_picture::PictureRgba {
                    width: w[0].value().unwrap() as i16,
                    height: w[1].value().unwrap() as i16,
                    ratio: 1.5,
                    fields: 3,
                    pad: 1,
                    data: c.input(px).to_bytes()[..4 * pixels].to_vec(),
                }
            };
            let ours = tile::tile_pic(&get("a", "pa"), &get("b", "pb"), &get("c", "pc"), &get("d", "pd"), c.input("across").i32(), c.input("up").i32());
            match ours {
                Some(p) => Verdict::from(c.output("eax").u32() != 0, || "the original returned nothing".into()).and(|| same_returned_rgba(c, &p)),
                None => Verdict::from(c.output("eax").u32() == 0, || "the port returned nothing".into()),
            }
        },
        "SI_generateTiledPic" => |c: &Case| {
            let w = c.input("rec").words();
            let pixels = w[0].value().unwrap() as usize * w[1].value().unwrap() as usize;
            let p = si_picture::PictureRgba {
                width: w[0].value().unwrap() as i16,
                height: w[1].value().unwrap() as i16,
                ratio: 1.5,
                fields: 3,
                pad: 1,
                data: c.input("px").to_bytes()[..4 * pixels].to_vec(),
            };
            let ours = tile::generate_tiled_pic(&p, c.input("across").i32(), c.input("up").i32(), c.input("mode").i16());
            match ours {
                Some(q) => Verdict::from(c.output("result").u32() == 1, || "the original left nothing".into()).and(|| same_returned_rgba(c, &q)),
                None => Verdict::from(c.output("result").u32() == 0, || "the port left nothing".into()),
            }
        },
        _ => return None,
    })
}
