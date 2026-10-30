// SPDX-License-Identifier: MIT
//! Checks for the compositor and its helpers (`si_imgfmt::composite`).

use crate::ops::same_picture_after;
use crate::util::*;
use si_file::Stream;
use si_imgfmt::composite::{self, Depth};
use si_picture::Picture;
use si_x87::vectors::{Case, Check, Verdict};

/// The two pictures of a case, each with the planes its record points to.
fn pair(c: &Case) -> (Picture, Picture) {
    (picture_in(c, c.input("dst").words()), picture_in(c, c.input("src").words()))
}

fn composite(c: &Case, depth: bool) -> Verdict {
    let (mut dst, src) = pair(c);
    let (x, y, mode, f) = (c.input("x").i16(), c.input("y").i16(), c.input("mode").i16(), c.input("f").f32());
    let (bz, lz) = if depth { (c.input("bz").to_bytes(), c.input("lz").to_bytes()) } else { (Vec::new(), Vec::new()) };
    let mut z = Depth { back: Stream::new(&bz), layer: Stream::new(&lz), out: Vec::new() };
    let with = depth && c.input("zf").i16() != 0;
    if composite::picture_composite(&mut dst, &src, x, y, mode, f, if with { Some(&mut z) } else { None }).is_err() {
        return Verdict::Skip;
    }
    let mut v = Verdict::from(c.output("eax").u32() == 0, || "the original returned something".into()).and(|| same_picture_after(c, c.after("dst").words(), &dst));
    if depth {
        v = v.and(|| same_bytes("new depth file", &c.try_output("nz").map(|f| f.to_bytes()).unwrap_or_default(), &z.out));
    }
    v
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "pictureComposite" => |c: &Case| composite(c, true),
        "compositePicture" => |c: &Case| composite(c, false),
        "pictureFade" | "fadePicture" => |c: &Case| {
            let (mut dst, src) = pair(c);
            let wrapper = c.name.starts_with("fade");
            let same_size = (dst.width, dst.height) == (src.width, src.height);
            let r = if wrapper { composite::fade_picture(&mut dst, &src, c.input("t").f32()) } else { composite::picture_fade(&mut dst, &src, c.input("t").f32()) };
            match r {
                Ok(result) => Verdict::from(c.output("eax").u32() == result as u32, || "result".into())
                    .and(|| {
                        let done = same_size;
                        let want: Vec<String> = if done { vec![] } else { vec![composite::FADE_MESSAGE.to_string()] };
                        Verdict::from(messages(c) == want, || format!("messages {:?}", messages(c)))
                    })
                    .and(|| same_picture_after(c, c.after("dst").words(), &dst)),
                Err(_) => Verdict::Skip,
            }
        },
        "setAlpha" => |c: &Case| {
            let (mut dst, src) = pair(c);
            match composite::set_alpha(&mut dst, &src, c.input("x").i32(), c.input("y").i32()) {
                Ok(()) => same_picture_after(c, c.after("dst").words(), &dst),
                Err(_) => Verdict::Skip,
            }
        },
        "superpose" => |c: &Case| {
            let p = picture_in(c, c.input("p").words());
            let ours = composite::superpose(c.input("x").i32(), c.input("y").i32(), &p);
            Verdict::from(c.output("eax").u32() == ours as u32, || format!("port {ours}"))
        },
        "layerBlur" => |c: &Case| {
            let mut dst = picture_in(c, c.input("dst").words());
            match composite::layer_blur(&mut dst, c.input("r").i32()) {
                Ok(()) => same_picture_after(c, c.after("dst").words(), &dst),
                Err(_) => Verdict::Skip,
            }
        },
        "layerGetContour" => |c: &Case| {
            let src = picture_in(c, c.input("src").words());
            let mut out = picture_in(c, c.input("out").words());
            let n = src.width as usize * src.height as usize;
            match composite::layer_get_contour(&src, &mut out, &c.input("mask").to_bytes()[..n], c.input("thr").i32(), c.input("nb").i32()) {
                Ok(()) => same_picture_after(c, c.after("out").words(), &out),
                Err(_) => Verdict::Skip,
            }
        },
        _ => return None,
    })
}
