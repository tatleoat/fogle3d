// SPDX-License-Identifier: MIT
//! Checks for the colour-index picture, its palette and their files (`si_imgfmt::dgt`).

use crate::util::*;
use si_imgfmt::dgt::{self, DgEntry, DgPalette, DgPicture};
use si_x87::vectors::{Case, Check, Verdict, Word};

fn val(w: &Word) -> u128 {
    w.value().unwrap_or(0)
}

fn bytes_of(c: &Case, w: &Word, n: usize) -> Option<Vec<u8>> {
    match w {
        Word::Ref { name, offset } => c.try_input(name).and_then(|f| f.to_bytes().get(*offset as usize..*offset as usize + n).map(|b| b.to_vec())),
        _ => None,
    }
}

/// A palette a case hands to the routine: the record `name` (23 words) with its entries and
/// tables taken from the buffers its pointers lead to.
pub fn pal_in(c: &Case, name: &str) -> DgPalette {
    pal_from(c, c.input(name).words())
}

pub fn pal_from(c: &Case, w: &[Word]) -> DgPalette {
    let count = val(&w[0]) as u32;
    let bits = [1, 2, 3, 4].map(|i| val(&w[i]) as u32 as i32);
    let entries = bytes_of(c, &w[10], 20 * count as usize).map(|b| b.chunks_exact(20).map(DgEntry::from_bytes).collect()).unwrap_or_default();
    let table = |i: usize, n: usize| bytes_of(c, &w[i], n);
    DgPalette {
        count,
        bits,
        max: [5, 6, 7, 8].map(|i| val(&w[i]) as u8),
        size: val(&w[9]) as u32 as i32,
        entries,
        to8: [0, 1, 2, 3].map(|k| table(12 + k, 1usize << (bits[k] & 31).min(16))),
        from8: [0, 1, 2, 3].map(|k| table(16 + k, 256)),
        sel: [20, 21, 22].map(|i| val(&w[i]) as u32 as i32),
    }
}

/// An index picture a case hands to the routine (record `dg`, palette record `pal`).
pub fn dg_in(c: &Case) -> DgPicture {
    let w = c.input("dg").words();
    let (width, height) = (val(&w[0]) as u16, val(&w[1]) as u16);
    let n = width as usize * height as usize;
    DgPicture {
        width,
        height,
        size: val(&w[2]) as u32,
        palette: matches!(w[3], Word::Ref { .. }).then(|| pal_in(c, "pal")),
        data: bytes_of(c, &w[4], 2 * n).map(|b| b.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect()),
        bytes: None,
        rect: [6, 7, 8, 9].map(|i| val(&w[i]) as u16),
    }
}

fn word_is(what: &str, w: &Word, ours: u128) -> Verdict {
    match w.value() {
        Some(v) if v != ours => Verdict::Different(format!("{what}: original {v:#x}, port {ours:#x}")),
        _ => Verdict::Same,
    }
}

/// Our entries against the bytes the original left: bytes it did not write are not
/// compared, nor are the bytes of an 8-bit word the port marks as undefined in the original.
fn same_entries(what: &str, theirs: &[Option<u128>], ours: &[DgEntry]) -> Verdict {
    let mut flip = mutate();
    for (i, e) in ours.iter().enumerate() {
        let b = e.to_bytes();
        for k in 0..20 {
            let Some(Some(t)) = theirs.get(20 * i + k) else { continue };
            if (4..8).contains(&k) && e.undefined >> (k - 4) & 1 != 0 {
                continue;
            }
            let mine = if flip { b[k] ^ 1 } else { b[k] };
            flip = false;
            if *t != mine as u128 {
                return Verdict::Different(format!("{what} {i} byte {k}: original {t:#x}, port {mine:#x}"));
            }
        }
    }
    Verdict::Same
}

/// Our palette against the record and buffers a post hook read (`<tag>pal`, `<tag>ent`,
/// `<tag>t0`…, `<tag>f0`…).
pub fn same_pal(c: &Case, tag: &str, ours: &DgPalette) -> Verdict {
    same_pal_upto(c, tag, ours, usize::MAX)
}

/// As [`same_pal`], comparing the first `limit` entries only.
pub fn same_pal_upto(c: &Case, tag: &str, ours: &DgPalette, limit: usize) -> Verdict {
    let Some(rec) = c.try_output(&format!("{tag}pal")) else { return Verdict::Different("the original left no palette".into()) };
    let w = rec.words();
    let mut v = word_is("count", &w[0], ours.count as u128);
    for k in 0..4 {
        v = v.and(|| word_is("depth", &w[1 + k], ours.bits[k] as u32 as u128)).and(|| word_is("largest value", &w[5 + k], ours.max[k] as u128));
    }
    v = v.and(|| word_is("size", &w[9], ours.size as u32 as u128));
    for k in 0..3 {
        v = v.and(|| word_is("entry number", &w[20 + k], ours.sel[k] as u32 as u128));
    }
    v = v.and(|| Verdict::from(matches!(w[10], Word::Ref { .. }) != ours.entries.is_empty(), || format!("entries pointer {:?}, port has {}", w[10], ours.entries.len())));
    v = v.and(|| match c.try_output(&format!("{tag}ent")) {
        Some(e) => same_entries("entry", &e.written(), &ours.entries[..(ours.count as usize).min(ours.entries.len()).min(limit)]),
        None => Verdict::from(ours.count == 0 || ours.entries.is_empty(), || "no entries in the original".into()),
    });
    for k in 0..4 {
        for (letter, t) in [("t", &ours.to8[k]), ("f", &ours.from8[k])] {
            v = v.and(|| match (c.try_output(&format!("{tag}{letter}{k}")), t) {
                (Some(f), Some(t)) => same_written(&format!("table {letter}{k}"), f, &wide(t)),
                (None, None) => Verdict::Same,
                (a, b) => Verdict::Different(format!("table {letter}{k}: original {}, port {}", a.is_some(), b.is_some())),
            });
        }
    }
    v
}

/// Our index picture against what a post hook read (`<tag>dg`, `<tag>ix`, `<tag>by`, and the
/// palette's fields).
pub fn same_dg(c: &Case, tag: &str, ours: &DgPicture) -> Verdict {
    let Some(rec) = c.try_output(&format!("{tag}dg")) else { return Verdict::Different("the original left no picture".into()) };
    let w = rec.words();
    let mut v = word_is("width", &w[0], ours.width as u128).and(|| word_is("height", &w[1], ours.height as u128)).and(|| word_is("size", &w[2], ours.size as u128));
    for k in 0..4 {
        v = v.and(|| word_is("rectangle", &w[6 + k], ours.rect[k] as u128));
    }
    v = v.and(|| Verdict::from(matches!(w[4], Word::Ref { .. }) == ours.data.is_some(), || format!("index buffer {:?}, port {}", w[4], ours.data.is_some())));
    v = v.and(|| Verdict::from(matches!(w[5], Word::Ref { .. }) == ours.bytes.is_some(), || format!("byte buffer {:?}, port {}", w[5], ours.bytes.is_some())));
    v = v.and(|| match (c.try_output(&format!("{tag}ix")), &ours.data) {
        (Some(f), Some(d)) => {
            let mut d: Vec<u128> = d.iter().map(|&x| x as u128).collect();
            d.truncate(f.len());
            same_written("indices", f, &d)
        }
        (None, _) => Verdict::Same,
        (Some(_), None) => Verdict::Different("indices in the original only".into()),
    });
    v = v.and(|| match (c.try_output(&format!("{tag}by")), &ours.bytes) {
        (Some(f), Some(d)) => same_written("byte buffer", f, &wide(&d[..f.len().min(d.len())])),
        _ => Verdict::Same,
    });
    v.and(|| match &ours.palette {
        Some(p) => same_pal(c, tag, p),
        None => Verdict::from(!matches!(w[3], Word::Ref { .. }), || "a palette in the original only".into()),
    })
}

fn i32_of(c: &Case, name: &str) -> i32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32 as i32
}

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

fn bits_of(c: &Case) -> [i32; 4] {
    c.input("bits").i32s().try_into().unwrap()
}

/// The entries buffer of an in-place call as the routine left it, against ours (entries
/// beyond the palette's count are the input's).
fn same_entry_buffer(c: &Case, p: &DgPalette) -> Verdict {
    let before = c.input("ent").to_bytes();
    let mut ours: Vec<DgEntry> = before.chunks_exact(20).map(DgEntry::from_bytes).collect();
    for (o, e) in ours.iter_mut().zip(&p.entries) {
        *o = *e;
    }
    same_entries("entry", &c.after("ent").written(), &ours)
}

fn text_of(c: &Case, name: &str) -> Option<Vec<u8>> {
    c.try_input(name).filter(|f| !f.is_null()).map(|f| f.to_bytes())
}

fn freed(c: &Case) -> usize {
    c.try_output("freed").map_or(0, |f| f.len())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_dgPaletteAllocate" => |c: &Case| Verdict::from(eax(c) == 1, || "no palette".into()).and(|| same_pal(c, "n", &DgPalette::allocate(i32_of(c, "n"), &bits_of(c)))),
        "SI_dgPaletteSetEntry" => |c: &Case| {
            let mut p = pal_in(c, "pal");
            match p.set_entry(c.input("i").u32(), ["r", "g", "b", "a"].map(|n| i32_of(c, n))) {
                Ok(()) => same_entry_buffer(c, &p),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_dgPaletteResize" => |c: &Case| {
            let mut p = pal_in(c, "pal");
            // entries the call adds are whatever `realloc` returns (in the harness: the rest
            // of the test buffer): not compared
            let old = p.count as usize;
            match p.resize(i32_of(c, "n")) {
                Ok(()) => same_pal_upto(c, "n", &p, old),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_DG_paletteAddEntries" => |c: &Case| {
            let mut p = pal_in(c, "pal");
            match p.add_entries(c.input("k").u32()) {
                Ok(()) => same_pal(c, "n", &p),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_DG_paletteRemoveEntry" => |c: &Case| {
            let mut p = pal_in(c, "pal");
            match p.remove_entry(c.input("i").u32()) {
                Ok(()) => same_pal(c, "n", &p),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_dgPaletteRemoveEntries" => |c: &Case| {
            let mut p = pal_in(c, "pal");
            let done = p.remove_entries(c.input("k").u32());
            Verdict::from((eax(c) == 1) == done, || format!("returned {}, port {done}", eax(c))).and(|| same_pal(c, "n", &p))
        },
        "SI_dgPaletteCopy" => |c: &Case| match pal_in(c, "pal").copy() {
            Ok(p) => same_pal(c, "n", &p),
            Err(_) => Verdict::Skip,
        },
        // the port has no routine for these two: the check is of what the finding says they do
        "SI_dgPaletteDispose" => |c: &Case| {
            let had = matches!(c.input("pp").words()[0], Word::Ref { .. });
            let n = pal_in(c, "pal").count;
            Verdict::from(c.after("pp").words()[0].value() == Some(0), || "the pointer was not cleared".into())
                .and(|| Verdict::from(freed(c) == if had { 9 + (n > 0) as usize } else { 0 }, || format!("{} blocks freed", freed(c))))
        },
        "SI_dgPictureDispose" => |c: &Case| {
            let had = matches!(c.input("pp").words()[0], Word::Ref { .. });
            Verdict::from(c.after("pp").words()[0].value() == Some(0), || "the pointer was not cleared".into())
                .and(|| Verdict::from(eax(c) == had as u32, || format!("returned {}", eax(c))))
                .and(|| Verdict::from(freed(c) == if had { 12 } else { 0 }, || format!("{} blocks freed", freed(c))))
        },
        "SI_changePaletteResolution" => |c: &Case| {
            let mut p = pal_in(c, "pal");
            match p.change_resolution(&bits_of(c)) {
                Ok(()) => same_pal(c, "n", &p),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_dgPictureAllocate" => |c: &Case| Verdict::from(eax(c) == 1, || "no picture".into()).and(|| same_dg(c, "n", &DgPicture::allocate(i32_of(c, "w"), i32_of(c, "h"), i32_of(c, "n"), &bits_of(c)))),
        "SI_dgPictureUpdateRefCount" => |c: &Case| {
            let mut d = dg_in(c);
            match d.update_ref_count() {
                Ok(()) => same_entry_buffer(c, d.palette.as_ref().unwrap()),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_dgScalePic" => |c: &Case| match dg_in(c).scale(i32_of(c, "w"), i32_of(c, "h")) {
            Ok(Some(d)) => Verdict::from(eax(c) == 1, || "no picture".into()).and(|| same_dg(c, "n", &d)),
            Ok(None) => Verdict::from(eax(c) == 0, || "a picture".into()),
            Err(_) => Verdict::Skip,
        },
        "SI_convertDG_Pict2SIPicture" => |c: &Case| match dg_in(c).to_picture() {
            Ok(Some(p)) => {
                let rec = c.output("pic").words();
                let mut v = Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_head(rec, p.width, p.height, p.ratio, p.fields, p.pad));
                for i in 0..4 {
                    v = v.and(|| match c.try_output(&format!("p{i}")) {
                        Some(d) => same_bytes(&format!("plane {i}"), &d.to_bytes(), p.planes[i].as_ref().unwrap()),
                        None => Verdict::Different(format!("no plane {i}")),
                    });
                }
                v
            }
            Ok(None) => Verdict::from(eax(c) == 0, || "the original succeeded".into()).and(|| Verdict::from((5..9).all(|i| c.output("pic").words()[i].value() == Some(0)), || "planes left".into())),
            Err(_) => Verdict::Skip,
        },
        "SI_convertDG_Pict2RGBA" => |c: &Case| match dg_in(c).to_rgba() {
            Ok(Some((p, _))) => Verdict::from(eax(c) == 1, || "the original failed".into())
                .and(|| same_head(c.output("out").words(), p.width, p.height, p.ratio, p.fields, p.pad))
                .and(|| same_bytes("pixels", &c.output("data").to_bytes(), &p.data)),
            Ok(None) => Verdict::from(eax(c) == 0, || "the original succeeded".into()),
            Err(_) => Verdict::Skip,
        },
        "SI_addExtensionToFilename" => |c: &Case| match dgt::add_extension_to_filename(text_of(c, "name").as_deref(), text_of(c, "ext").as_deref()) {
            Some(t) => Verdict::from(eax(c) == 1, || "nothing returned".into()).and(|| same_bytes("name", &c.output("text").to_bytes(), &t)),
            None => Verdict::from(eax(c) == 0, || "a name returned".into()),
        },
        "SI_writeDGTpalette" => |c: &Case| match dgt::write_dgt_palette(&dg_in(c)) {
            Ok(b) => same_bytes("file", &file_out(c, "/t/o.dgt").unwrap_or_default(), &b),
            Err(_) => Verdict::Skip,
        },
        "SI_writeDGTfile" => |c: &Case| match dgt::write_dgt_file(&dg_in(c)) {
            Ok(b) => same_bytes("file", &file_out(c, "/t/o.dgt").unwrap_or_default(), &b),
            Err(_) => Verdict::Skip,
        },
        "SI_readDGTfile" => |c: &Case| {
            let name = c.input("name").to_bytes();
            let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
            match dgt::read_dgt_file(&name, file.as_deref(), i32_of(c, "n")) {
                Ok(Some(d)) => Verdict::from(eax(c) == 1, || "no picture".into()).and(|| same_dg(c, "n", &d)),
                Ok(None) => Verdict::from(eax(c) == 0, || "a picture".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_userPalReadFile" => |c: &Case| {
            let name = c.input("name").to_bytes();
            let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
            match dgt::read_user_pal(file.as_deref()) {
                Ok(Some(p)) => Verdict::from(eax(c) == 1, || "no palette".into()).and(|| same_pal(c, "n", &p)),
                Ok(None) => Verdict::from(eax(c) == 0, || "a palette".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_userPalWriteFile" => |c: &Case| {
            if c.input("name").is_null() {
                return Verdict::from(eax(c) == 0 && file_out(c, "/t/o.pal").is_none(), || "written without a name".into());
            }
            match dgt::write_user_pal(&pal_in(c, "pal")) {
                Ok(b) => Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_bytes("file", &file_out(c, "/t/o.pal").unwrap_or_default(), &b)),
                Err(_) => Verdict::Skip,
            }
        },
        _ => return None,
    })
}
