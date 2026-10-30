// SPDX-License-Identifier: MIT
//! What the checks share: reading the records and buffers of a case.

use si_picture::{Picture, PictureRgba};
use si_x87::vectors::{Case, Field, Verdict, Word};

/// The bytes of an in-memory file handed to the routine.
pub fn file_in(c: &Case, path: &str) -> Vec<u8> {
    c.input(&format!("file:{path}")).to_bytes()
}

/// The bytes of a file the routine wrote, if it opened one.
pub fn file_out(c: &Case, path: &str) -> Option<Vec<u8>> {
    c.try_output(&format!("file:{path}")).map(|f| f.to_bytes())
}

pub fn text(f: &Field) -> String {
    String::from_utf8_lossy(&f.to_bytes()).into_owned()
}

/// The messages the routine showed, in order (the stub of `SI_statusDisplayMessage`).
pub fn messages(c: &Case) -> Vec<String> {
    c.outputs.iter().filter(|f| f.name.starts_with("str")).map(text).collect()
}

fn value(w: &Word) -> u128 {
    w.value().unwrap_or(0)
}

/// The buffer a pointer word of a record leads to: `None` for a null pointer.
pub fn target<'a>(c: &'a Case, w: &Word) -> Option<(&'a Field, usize)> {
    match w {
        Word::Ref { name, offset } => c.try_output(name).or_else(|| c.try_input(name)).map(|f| (f, *offset as usize)),
        _ => None,
    }
}

pub fn bytes_at(c: &Case, w: &Word, n: usize) -> Option<Vec<u8>> {
    let (f, off) = target(c, w)?;
    let b = f.to_bytes();
    Some(b.get(off..off + n)?.to_vec())
}

/// An `SI_Picture` record of a case (nine words), with its planes taken from the buffers
/// its pointers lead to.
pub fn picture(c: &Case, words: &[Word]) -> Picture {
    let (w, h) = (value(&words[0]) as u16 as i16, value(&words[1]) as u16 as i16);
    let n = (w.max(0) as usize) * (h.max(0) as usize);
    let plane = |i: usize| bytes_at(c, &words[5 + i], n);
    Picture {
        width: w,
        height: h,
        ratio: f32::from_bits(value(&words[2]) as u32),
        fields: value(&words[3]) as u16 as i16,
        pad: value(&words[4]) as u16 as i16,
        planes: [plane(0), plane(1), plane(2), plane(3)],
    }
}

/// An `SI_PictureRGBA` record (six words) with `per_pixel` bytes a pixel in its buffer.
pub fn rgba(c: &Case, words: &[Word], pixels: usize, per_pixel: usize) -> Option<PictureRgba> {
    Some(PictureRgba {
        width: value(&words[0]) as u16 as i16,
        height: value(&words[1]) as u16 as i16,
        ratio: f32::from_bits(value(&words[2]) as u32),
        fields: value(&words[3]) as u16 as i16,
        pad: value(&words[4]) as u16 as i16,
        data: bytes_at(c, &words[5], pixels * per_pixel)?,
    })
}

/// Compares the header words the routine wrote (unwritten ones are skipped) with ours.
pub fn same_head(words: &[Word], w: i16, h: i16, ratio: f32, fields: i16, pad: i16) -> Verdict {
    let ours = [w as u16 as u128, h as u16 as u128, ratio.to_bits() as u128, fields as u16 as u128, pad as u16 as u128];
    for (i, (theirs, ours)) in words.iter().zip(ours).enumerate() {
        if let Word::Value { value, .. } = theirs {
            if *value != ours {
                return Verdict::Different(format!("header word {i}: original {value:#x}, port {ours:#x}"));
            }
        }
    }
    Verdict::Same
}

/// The self-test switch: with `EMUCHECK_MUTATE=1` in the environment one bit of what the
/// port produced is flipped before the byte, word and plane comparisons, so that every case
/// that compares any such output must come out different. Shows that the comparisons are live.
pub fn mutate() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "1"))
}

pub fn same_bytes(what: &str, theirs: &[u8], ours: &[u8]) -> Verdict {
    if mutate() && !ours.is_empty() {
        let mut flipped = ours.to_vec();
        flipped[0] ^= 1;
        return if theirs == flipped { Verdict::Same } else { Verdict::Different(format!("{what}: mutated")) };
    }
    if theirs == ours {
        return Verdict::Same;
    }
    let at = theirs.iter().zip(ours).position(|(a, b)| a != b).unwrap_or(theirs.len().min(ours.len()));
    Verdict::Different(format!("{what}: lengths {} and {}, first difference at {at}", theirs.len(), ours.len()))
}

/// Compares a picture the routine left (record words) with ours, planes included.
pub fn same_picture(c: &Case, words: &[Word], ours: &Picture) -> Verdict {
    let mut v = same_head(words, ours.width, ours.height, ours.ratio, ours.fields, ours.pad);
    let n = (ours.width.max(0) as usize) * (ours.height.max(0) as usize);
    for i in 0..4 {
        v = v.and(|| match (&ours.planes[i], bytes_at(c, &words[5 + i], n)) {
            (None, None) => Verdict::Same,
            (Some(a), Some(b)) => same_bytes(&format!("plane {i}"), &b, a),
            (a, b) => Verdict::Different(format!("plane {i}: port {}, original {}", a.is_some(), b.is_some())),
        });
    }
    v
}

pub fn same_rgba(c: &Case, words: &[Word], ours: &PictureRgba) -> Verdict {
    same_head(words, ours.width, ours.height, ours.ratio, ours.fields, ours.pad).and(|| match bytes_at(c, &words[5], ours.data.len()) {
        Some(b) => same_bytes("pixels", &b, &ours.data),
        None => Verdict::Different("the original left no pixel buffer of our size".into()),
    })
}

/// Compares the words the routine wrote with ours; words it left unwritten (memory it
/// allocated and did not fill, or filled from memory it never set) are not compared.
pub fn same_written(what: &str, theirs: &Field, ours: &[u128]) -> Verdict {
    let w = theirs.written();
    if w.len() != ours.len() {
        return Verdict::Different(format!("{what}: {} words against {}", w.len(), ours.len()));
    }
    if mutate() && w.first().is_some_and(|x| x.is_some()) {
        return Verdict::Different(format!("{what}: mutated"));
    }
    match w.iter().zip(ours).position(|(a, b)| a.is_some_and(|a| a != *b)) {
        Some(at) => Verdict::Different(format!("{what}: word {at} is {:#x}, port {:#x}", w[at].unwrap(), ours[at])),
        None => Verdict::Same,
    }
}

pub fn wide(bytes: &[u8]) -> Vec<u128> {
    bytes.iter().map(|&b| b as u128).collect()
}

/// A returned packed picture (`rec` and `data` of the generator's post hook) against ours.
pub fn same_returned_rgba(c: &Case, ours: &PictureRgba) -> Verdict {
    let rec = c.output("rec").words();
    same_head(rec, ours.width, ours.height, ours.ratio, ours.fields, ours.pad).and(|| match c.try_output("data") {
        Some(d) => same_written("pixels", d, &wide(&ours.data)),
        None => Verdict::from(ours.data.is_empty(), || "the original has no pixels".into()),
    })
}

/// A returned plane picture (`rec`, `p0`…`p3`) against ours.
pub fn same_returned_picture(c: &Case, ours: &Picture) -> Verdict {
    let mut v = same_head(c.output("rec").words(), ours.width, ours.height, ours.ratio, ours.fields, ours.pad);
    for i in 0..4 {
        v = v.and(|| match (c.try_output(&format!("p{i}")), &ours.planes[i]) {
            (Some(d), Some(p)) => same_written(&format!("plane {i}"), d, &wide(p)),
            (None, Some(p)) => Verdict::from(p.is_empty(), || format!("plane {i} missing in the original")),
            (Some(_), None) => Verdict::Different(format!("plane {i} missing in the port")),
            (None, None) => Verdict::Same,
        });
    }
    v
}

pub fn cstr(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

/// A picture the routine was handed: as [`picture`], with the planes as they were before the
/// call (a routine working in place changes the same buffers).
pub fn picture_in(c: &Case, words: &[Word]) -> Picture {
    let mut p = picture(c, words);
    let n = (p.width.max(0) as usize) * (p.height.max(0) as usize);
    for i in 0..4 {
        p.planes[i] = match &words[5 + i] {
            Word::Ref { name, offset } => c.try_input(name).and_then(|f| f.to_bytes().get(*offset as usize..*offset as usize + n).map(|b| b.to_vec())),
            _ => None,
        };
    }
    p
}
