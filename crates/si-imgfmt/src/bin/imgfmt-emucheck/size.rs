// SPDX-License-Identifier: MIT
//! Checks for the resizing operations (`si_imgfmt::size`).

use crate::ops::same_picture_after;
use crate::util::*;
use si_imgfmt::effect::Fault;
use si_imgfmt::size;
use si_picture::{Picture, PictureRgba};
use si_x87::vectors::{Case, Check, Verdict, Word};

/// An operation in place on `pic` that returns 1 or 0: the result, the record and the planes
/// afterwards against ours. Where the port stops because the original would use memory that
/// is not its own, nothing is compared.
pub fn in_place(c: &Case, op: impl FnOnce(&mut Picture) -> Result<bool, Fault>) -> Verdict {
    let mut p = picture_in(c, c.input("pic").words());
    match op(&mut p) {
        Ok(r) => Verdict::from(c.output("eax").u32() == r as u32, || format!("the original returned {}, the port {r}", c.output("eax").u32()))
            .and(|| same_picture_after(c, c.after("pic").words(), &p)),
        Err(_) => Verdict::Skip,
    }
}

fn word(w: &Word) -> u128 {
    w.value().unwrap_or(0)
}

/// One of the three whole-step scales of the packed records.
fn packed(c: &Case, per: usize, half: bool, op: fn(&mut PictureRgba, &PictureRgba, f32, f32) -> Result<(), Fault>) -> Verdict {
    let sw = c.input("src").words();
    let (w, h) = (word(&sw[0]) as u16 as i16, word(&sw[1]) as u16 as i16);
    let pixels = if half { (w / 2) as usize * (h / 2) as usize } else { w as usize * h as usize };
    let src = PictureRgba { width: w, height: h, ratio: 1.5, fields: 3, pad: 1, data: c.input("px").to_bytes()[..pixels * per].to_vec() };
    let dw = c.input("dst").words();
    let had = matches!(dw[5], Word::Ref { .. });
    let old = if had { c.input("old").to_bytes() } else { Vec::new() };
    let mut dst = PictureRgba {
        width: word(&dw[0]) as u16 as i16,
        height: word(&dw[1]) as u16 as i16,
        ratio: f32::from_bits(word(&dw[2]) as u32),
        fields: word(&dw[3]) as u16 as i16,
        pad: word(&dw[4]) as u16 as i16,
        data: old.clone(),
    };
    if op(&mut dst, &src, c.input("zx").f32(), c.input("zy").f32()).is_err() {
        return Verdict::Skip;
    }
    let after = c.after("dst").words();
    same_head(after, dst.width, dst.height, dst.ratio, dst.fields, dst.pad).and(|| match c.try_output("data") {
        Some(d) => same_written("pixels", d, &wide(&dst.data)),
        None => match &after[5] {
            Word::Ref { name, .. } if name == "old" => Verdict::from(dst.data == old && had, || "the original kept the old buffer".into()),
            Word::Ref { .. } => Verdict::Different("the original made a buffer the generator did not report".into()),
            _ => Verdict::from(dst.data.is_empty(), || "the original left no buffer".into()),
        },
    })
}

/// `pictureExtent`: the bordered copy it returns.
fn extent(c: &Case) -> Verdict {
    let (w, h) = (c.input("w").u32() as i32, c.input("h").u32() as i32);
    let px = c.input("px").to_bytes();
    match size::picture_extent(&px, w, h) {
        Ok(Some(d)) => match c.try_output("ext") {
            Some(o) => same_written("bordered copy", o, &wide(&d)),
            None => Verdict::Different("the original returned no copy".into()),
        },
        Ok(None) => Verdict::from(c.try_output("ext").is_none(), || "the original made a copy".into()),
        Err(_) => Verdict::Skip,
    }
}

/// `pictureRGBABilinearZoom` on a record the caller passed.
fn rgba_zoom(c: &Case) -> Verdict {
    let pw = c.input("p").words();
    let (w, h) = (word(&pw[0]) as u16 as i16, word(&pw[1]) as u16 as i16);
    let mut p = PictureRgba {
        width: w,
        height: h,
        ratio: f32::from_bits(word(&pw[2]) as u32),
        fields: word(&pw[3]) as u16 as i16,
        pad: word(&pw[4]) as u16 as i16,
        data: c.input("px").to_bytes()[..w as usize * h as usize * 4].to_vec(),
    };
    let (zx, zy) = (c.input("zx").f32(), c.input("zy").f32());
    if size::rgba_bilinear_zoom(&mut p, zx, zy).is_err() {
        return Verdict::Skip;
    }
    let after = c.after("p").words();
    let unchanged = zx == 1.0 && zy == 1.0;
    same_head(after, p.width, p.height, p.ratio, p.fields, p.pad).and(|| match c.try_output("data") {
        _ if unchanged => Verdict::from(matches!(after[5], Word::Ref { .. }) && p.data.len() == w as usize * h as usize * 4, || "the buffer of an unchanged record moved".into()),
        Some(d) => same_written("pixels", d, &wide(&p.data)),
        None => Verdict::from(p.data.is_empty() || p.width as i32 * p.height as i32 == 0, || "the original left no buffer".into()),
    })
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureAverage" => |c: &Case| in_place(c, |p| size::average(p, c.input("fx").i16(), c.input("fy").i16())),
        "SI_pictureEnlarge" => |c: &Case| in_place(c, |p| size::enlarge(p, c.input("fx").i16(), c.input("fy").i16())),
        "SI_pictureBilinearZoom" => |c: &Case| in_place(c, |p| size::bilinear_zoom(p, c.input("zx").f32(), c.input("zy").f32())),
        "SI_pictureScale" => |c: &Case| in_place(c, |p| size::scale(p, c.input("sx").f32(), c.input("sy").f32())),
        "SI_scalePictureX" => |c: &Case| in_place(c, |p| size::scale_picture_x(p, c.input("ratio").f32())),
        "SI_pictureRGBAIntegerScale" => |c: &Case| packed(c, 4, false, size::rgba_integer_scale),
        "SI_pictureBWIntegerScale" => |c: &Case| packed(c, 1, false, size::bw_integer_scale),
        "SI_pictureScaledRGBAIntegerScale" => |c: &Case| packed(c, 4, true, size::scaled_rgba_integer_scale),
        "pictureExtent" => extent,
        "pictureRGBABilinearZoom" => rgba_zoom,
        _ => return None,
    })
}
